use crate::define_repo_test;
use ps_core::ingestion::ContributionInput;
use ps_core::models::{ContributionState, ContributionType, PeriodType, Platform, TeamType};
use time::OffsetDateTime;
use uuid::Uuid;

async fn insert_person(pool: &sqlx::PgPool, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO org.people (id, name) VALUES ($1, $2)")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .expect("insert person");
    id
}

fn make_pr(
    platform_id: &str,
    state: ContributionState,
    created_at: OffsetDateTime,
    closed_at: Option<OffsetDateTime>,
) -> ContributionInput {
    ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PullRequest,
        platform_id: platform_id.into(),
        platform_username: "testuser".into(),
        title: Some(format!("PR {platform_id}")),
        url: None,
        state: Some(state),
        created_at,
        updated_at: None,
        closed_at,
        metrics: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }
}

fn make_jira_ticket(
    platform_id: &str,
    state: ContributionState,
    created_at: OffsetDateTime,
    closed_at: Option<OffsetDateTime>,
    cycle_time_hours: Option<f64>,
) -> ContributionInput {
    let mut metrics = serde_json::json!({});
    if let Some(hours) = cycle_time_hours {
        metrics["cycle_time_hours"] = serde_json::json!(hours);
    }
    ContributionInput {
        platform: Platform::Jira,
        contribution_type: ContributionType::JiraTicket,
        platform_id: platform_id.into(),
        platform_username: "testuser".into(),
        title: Some(format!("JIRA {platform_id}")),
        url: None,
        state: Some(state),
        created_at,
        updated_at: None,
        closed_at,
        metrics,
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }
}

/// Seed a team with one person who has the given contributions.
async fn seed_team(
    repos: &ps_core::repo::Repos,
    pool: &sqlx::PgPool,
    team_name: &str,
    person_name: &str,
    contributions: &[ContributionInput],
) -> Uuid {
    let team = repos
        .org
        .create_team(team_name, "TestOrg", TeamType::Team, None, None)
        .await
        .unwrap();
    let person = insert_person(pool, person_name).await;

    for item in contributions {
        repos
            .activity
            .upsert_contribution(Uuid::now_v7(), Some(person), item)
            .await
            .unwrap();
    }

    repos
        .org
        .assign_person_to_team(person.into(), team.id.into())
        .await
        .unwrap();

    team.id
}

// ---------------------------------------------------------------------------
// End-to-end snapshot computation for a week
// ---------------------------------------------------------------------------

define_repo_test!(compute_week_snapshot, |repos, pool| async move {
    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let jan_12 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        make_pr("PR-S-1", ContributionState::Merged, jan_10, Some(jan_12)),
        make_pr("PR-S-2", ContributionState::Merged, jan_10, Some(jan_12)),
        make_jira_ticket(
            "JIRA-S-1",
            ContributionState::Closed,
            jan_10,
            Some(jan_12),
            Some(24.0),
        ),
    ];

    let team_id = seed_team(&repos, &pool, "SnapTeam", "Alice", &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap();

    ps_metrics::compute_team_snapshot(&repos, team_id, period_start, period_end, PeriodType::Week)
        .await
        .unwrap();

    let snap = repos
        .metrics
        .get_team_snapshot(team_id, period_start, PeriodType::Week)
        .await
        .unwrap()
        .expect("snapshot should exist");

    assert_eq!(snap.throughput, Some(3)); // 2 merged PRs + 1 closed Jira
    assert!(snap.avg_cycle_time_hours.is_some()); // Jira cycle time
    assert!((snap.avg_cycle_time_hours.unwrap() - 24.0).abs() < 0.01);
});

// ---------------------------------------------------------------------------
// Monthly period boundaries
// ---------------------------------------------------------------------------

define_repo_test!(compute_month_snapshot, |repos, pool| async move {
    let mar_15 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::March, 15).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let mar_20 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::March, 20).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![make_pr(
        "PR-M-1",
        ContributionState::Merged,
        mar_15,
        Some(mar_20),
    )];

    let team_id = seed_team(&repos, &pool, "MonthTeam", "Bob", &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::March, 1).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::March, 31).unwrap();

    ps_metrics::compute_team_snapshot(&repos, team_id, period_start, period_end, PeriodType::Month)
        .await
        .unwrap();

    let snap = repos
        .metrics
        .get_team_snapshot(team_id, period_start, PeriodType::Month)
        .await
        .unwrap()
        .expect("monthly snapshot should exist");

    assert_eq!(snap.throughput, Some(1));
    assert_eq!(snap.period_type, PeriodType::Month);
});

// ---------------------------------------------------------------------------
// Quarterly period boundaries
// ---------------------------------------------------------------------------

define_repo_test!(compute_quarter_snapshot, |repos, pool| async move {
    let feb_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::February, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let feb_15 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::February, 15).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        make_pr("PR-Q-1", ContributionState::Merged, feb_10, Some(feb_15)),
        make_pr("PR-Q-2", ContributionState::Merged, feb_10, Some(feb_15)),
    ];

    let team_id = seed_team(&repos, &pool, "QuarterTeam", "Carol", &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 1).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::March, 31).unwrap();

    ps_metrics::compute_team_snapshot(
        &repos,
        team_id,
        period_start,
        period_end,
        PeriodType::Quarter,
    )
    .await
    .unwrap();

    let snap = repos
        .metrics
        .get_team_snapshot(team_id, period_start, PeriodType::Quarter)
        .await
        .unwrap()
        .expect("quarterly snapshot should exist");

    assert_eq!(snap.throughput, Some(2));
    assert_eq!(snap.period_type, PeriodType::Quarter);
});

// ---------------------------------------------------------------------------
// Recompute overwrites stale snapshot
// ---------------------------------------------------------------------------

define_repo_test!(recompute_overwrites_stale, |repos, pool| async move {
    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let jan_12 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    // Start with 1 merged PR
    let items = vec![make_pr(
        "PR-R-1",
        ContributionState::Merged,
        jan_10,
        Some(jan_12),
    )];

    let team_id = seed_team(&repos, &pool, "RecomputeTeam", "Dave", &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap();

    ps_metrics::compute_team_snapshot(&repos, team_id, period_start, period_end, PeriodType::Week)
        .await
        .unwrap();

    let snap1 = repos
        .metrics
        .get_team_snapshot(team_id, period_start, PeriodType::Week)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snap1.throughput, Some(1));

    // Add another merged PR and recompute
    let new_pr = make_pr("PR-R-2", ContributionState::Merged, jan_10, Some(jan_12));
    // Need to get person_id from existing contribution
    let contribs = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await
        .unwrap();
    let person_id = contribs[0].person_id;

    repos
        .activity
        .upsert_contribution(Uuid::now_v7(), person_id, &new_pr)
        .await
        .unwrap();

    ps_metrics::compute_team_snapshot(&repos, team_id, period_start, period_end, PeriodType::Week)
        .await
        .unwrap();

    let snap2 = repos
        .metrics
        .get_team_snapshot(team_id, period_start, PeriodType::Week)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snap2.throughput, Some(2));
});

// ---------------------------------------------------------------------------
// Snapshot includes per-source counts in raw_metrics
// ---------------------------------------------------------------------------

define_repo_test!(snapshot_includes_source_counts, |repos, pool| async move {
    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let jan_12 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        make_pr("PR-SC-1", ContributionState::Merged, jan_10, Some(jan_12)),
        make_pr("PR-SC-2", ContributionState::Merged, jan_10, Some(jan_12)),
        make_jira_ticket(
            "JIRA-SC-1",
            ContributionState::Closed,
            jan_10,
            Some(jan_12),
            Some(24.0),
        ),
    ];

    let team_id = seed_team(&repos, &pool, "SourceCountTeam", "Eve", &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap();

    ps_metrics::compute_team_snapshot(&repos, team_id, period_start, period_end, PeriodType::Week)
        .await
        .unwrap();

    let snap = repos
        .metrics
        .get_team_snapshot(team_id, period_start, PeriodType::Week)
        .await
        .unwrap()
        .unwrap();

    // raw_metrics should contain throughput_by_source
    let raw: serde_json::Value = snap.raw_metrics;
    let by_source = &raw["throughput_by_source"];
    assert_eq!(by_source["github"], 2);
    assert_eq!(by_source["jira"], 1);
});

// ---------------------------------------------------------------------------
// Multi-team computation via compute_all_snapshots
// ---------------------------------------------------------------------------

define_repo_test!(multi_team_computation, |repos, pool| async move {
    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let jan_12 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let team_a = seed_team(
        &repos,
        &pool,
        "TeamA",
        "Alice",
        &[make_pr(
            "PR-MA-1",
            ContributionState::Merged,
            jan_10,
            Some(jan_12),
        )],
    )
    .await;

    let team_b = seed_team(
        &repos,
        &pool,
        "TeamB",
        "Bob",
        &[
            make_pr("PR-MB-1", ContributionState::Merged, jan_10, Some(jan_12)),
            make_pr("PR-MB-2", ContributionState::Merged, jan_10, Some(jan_12)),
        ],
    )
    .await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap();

    let computed =
        ps_metrics::compute_all_snapshots(&repos, period_start, period_end, PeriodType::Week)
            .await
            .unwrap();
    assert_eq!(computed, 2);

    let snap_a = repos
        .metrics
        .get_team_snapshot(team_a, period_start, PeriodType::Week)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snap_a.throughput, Some(1));

    let snap_b = repos
        .metrics
        .get_team_snapshot(team_b, period_start, PeriodType::Week)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snap_b.throughput, Some(2));
});
