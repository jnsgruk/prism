use crate::define_repo_test;
use ps_core::ingestion::ContributionInput;
use ps_core::models::{ContributionState, ContributionType, PeriodType, Platform, TeamType};
use ps_core::repo::metrics::SnapshotInput;
use time::OffsetDateTime;
use uuid::Uuid;

/// Build a contribution with a specific created_at date.
fn make_contribution_at(
    platform: Platform,
    ctype: ContributionType,
    platform_id: &str,
    created_at: OffsetDateTime,
) -> ContributionInput {
    ContributionInput {
        platform,
        contribution_type: ctype,
        platform_id: platform_id.into(),
        platform_username: "testuser".into(),
        title: Some(format!("Test {platform_id}")),
        url: None,
        state: Some(ContributionState::Merged),
        created_at,
        updated_at: None,
        closed_at: None,
        metrics: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }
}

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

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

define_repo_test!(upsert_snapshot_and_retrieve, |repos, _pool| async move {
    let team = repos
        .org
        .create_team("Kernel", "Canonical", TeamType::Team, None, None)
        .await
        .unwrap();

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap();

    let snap = SnapshotInput {
        id: Uuid::now_v7(),
        team_id: team.id,
        period_start,
        period_end,
        period_type: PeriodType::Week,
        throughput: 15,
        avg_review_turnaround_hours: Some(4.5),
        avg_cycle_time_hours: Some(24.0),
        wip_avg: Some(3.2),
        flow_efficiency: Some(0.65),
        lead_time_hours: Some(48.0),
        raw_metrics: serde_json::json!({"custom": "data"}),
    };

    let returned_id = repos.metrics.upsert_snapshot(&snap).await.unwrap();

    let fetched = repos
        .metrics
        .get_team_snapshot(team.id, period_start, PeriodType::Week)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.id, returned_id);
    assert_eq!(fetched.throughput, Some(15));
    assert!((fetched.avg_review_turnaround_hours.unwrap() - 4.5).abs() < 0.01);
    assert_eq!(fetched.period_type, PeriodType::Week);
});

define_repo_test!(
    recompute_overwrites_stale_snapshot,
    |repos, _pool| async move {
        let team = repos
            .org
            .create_team("T", "O", TeamType::Team, None, None)
            .await
            .unwrap();
        let start = time::Date::from_calendar_date(2025, time::Month::March, 3).unwrap();
        let end = time::Date::from_calendar_date(2025, time::Month::March, 9).unwrap();

        let snap1 = SnapshotInput {
            id: Uuid::now_v7(),
            team_id: team.id,
            period_start: start,
            period_end: end,
            period_type: PeriodType::Week,
            throughput: 10,
            avg_review_turnaround_hours: None,
            avg_cycle_time_hours: None,
            wip_avg: None,
            flow_efficiency: None,
            lead_time_hours: None,
            raw_metrics: serde_json::json!({}),
        };
        repos.metrics.upsert_snapshot(&snap1).await.unwrap();

        // Recompute with updated throughput
        let snap2 = SnapshotInput {
            id: Uuid::now_v7(),
            team_id: team.id,
            period_start: start,
            period_end: end,
            period_type: PeriodType::Week,
            throughput: 20,
            avg_review_turnaround_hours: None,
            avg_cycle_time_hours: None,
            wip_avg: None,
            flow_efficiency: None,
            lead_time_hours: None,
            raw_metrics: serde_json::json!({}),
        };
        repos.metrics.upsert_snapshot(&snap2).await.unwrap();

        let fetched = repos
            .metrics
            .get_team_snapshot(team.id, start, PeriodType::Week)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.throughput, Some(20));
    }
);

define_repo_test!(compare_team_snapshots, |repos, _pool| async move {
    let t1 = repos
        .org
        .create_team("A", "O", TeamType::Team, None, None)
        .await
        .unwrap();
    let t2 = repos
        .org
        .create_team("B", "O", TeamType::Team, None, None)
        .await
        .unwrap();

    let start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let end = time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap();

    for (team_id, tp) in [(t1.id, 5), (t2.id, 12)] {
        let snap = SnapshotInput {
            id: Uuid::now_v7(),
            team_id,
            period_start: start,
            period_end: end,
            period_type: PeriodType::Week,
            throughput: tp,
            avg_review_turnaround_hours: None,
            avg_cycle_time_hours: None,
            wip_avg: None,
            flow_efficiency: None,
            lead_time_hours: None,
            raw_metrics: serde_json::json!({}),
        };
        repos.metrics.upsert_snapshot(&snap).await.unwrap();
    }

    let snapshots = repos
        .metrics
        .compare_team_snapshots(&[t1.id, t2.id], start, PeriodType::Week)
        .await
        .unwrap();
    assert_eq!(snapshots.len(), 2);
    // Ordered by team name
    assert_eq!(snapshots[0].team_name, "A");
    assert_eq!(snapshots[1].team_name, "B");
});

define_repo_test!(list_periods, |repos, _pool| async move {
    let team = repos
        .org
        .create_team("T", "O", TeamType::Team, None, None)
        .await
        .unwrap();

    // Empty initially
    assert!(repos.metrics.list_periods().await.unwrap().is_empty());

    let snap = SnapshotInput {
        id: Uuid::now_v7(),
        team_id: team.id,
        period_start: time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap(),
        period_end: time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap(),
        period_type: PeriodType::Week,
        throughput: 1,
        avg_review_turnaround_hours: None,
        avg_cycle_time_hours: None,
        wip_avg: None,
        flow_efficiency: None,
        lead_time_hours: None,
        raw_metrics: serde_json::json!({}),
    };
    repos.metrics.upsert_snapshot(&snap).await.unwrap();

    let periods = repos.metrics.list_periods().await.unwrap();
    assert_eq!(periods.len(), 1);
    assert_eq!(periods[0].period_type, PeriodType::Week);
});

define_repo_test!(get_snapshot_history, |repos, _pool| async move {
    let team = repos
        .org
        .create_team("T", "O", TeamType::Team, None, None)
        .await
        .unwrap();

    // Create 3 weekly snapshots
    for week in 0i32..3 {
        let start =
            time::Date::from_calendar_date(2025, time::Month::January, 6 + (week as u8) * 7)
                .unwrap();
        let end = time::Date::from_calendar_date(2025, time::Month::January, 12 + (week as u8) * 7)
            .unwrap();
        let snap = SnapshotInput {
            id: Uuid::now_v7(),
            team_id: team.id,
            period_start: start,
            period_end: end,
            period_type: PeriodType::Week,
            throughput: (week + 1) * 5,
            avg_review_turnaround_hours: None,
            avg_cycle_time_hours: None,
            wip_avg: None,
            flow_efficiency: None,
            lead_time_hours: None,
            raw_metrics: serde_json::json!({}),
        };
        repos.metrics.upsert_snapshot(&snap).await.unwrap();
    }

    let history = repos
        .metrics
        .get_snapshot_history(team.id, PeriodType::Week, 2)
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    // Ordered by period_start DESC
    assert!(history[0].period_start > history[1].period_start);
});

// ---------------------------------------------------------------------------
// Source contributions
// ---------------------------------------------------------------------------

define_repo_test!(
    get_team_contributions_for_period,
    |repos, pool| async move {
        let team = repos
            .org
            .create_team("T", "O", TeamType::Team, None, None)
            .await
            .unwrap();
        let alice = insert_person(&pool, "Alice").await;

        // Insert contributions BEFORE assigning to team — assign_person_to_team
        // sets start_date = MIN(created_at)::date from contributions, so the
        // contributions must exist first for the membership to cover the period.
        let jan_10 = time::OffsetDateTime::new_utc(
            time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
            time::Time::from_hms(12, 0, 0).unwrap(),
        );
        let item = make_contribution_at(
            Platform::Github,
            ContributionType::PullRequest,
            "period-1",
            jan_10,
        );
        repos
            .activity
            .upsert_contribution(Uuid::now_v7(), Some(alice), &item)
            .await
            .unwrap();

        let feb_10 = time::OffsetDateTime::new_utc(
            time::Date::from_calendar_date(2025, time::Month::February, 10).unwrap(),
            time::Time::from_hms(12, 0, 0).unwrap(),
        );
        let item2 = make_contribution_at(
            Platform::Github,
            ContributionType::PullRequest,
            "period-2",
            feb_10,
        );
        repos
            .activity
            .upsert_contribution(Uuid::now_v7(), Some(alice), &item2)
            .await
            .unwrap();

        // Now assign — start_date will be 2025-01-10 (earliest contribution).
        repos
            .org
            .assign_person_to_team(alice.into(), team.id.into())
            .await
            .unwrap();

        let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
        let period_end = time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap();

        let contribs = repos
            .metrics
            .get_team_contributions(team.id, period_start, period_end)
            .await
            .unwrap();
        assert_eq!(contribs.len(), 1);
        assert_eq!(contribs[0].platform_id, "period-1");
    }
);

// ---------------------------------------------------------------------------
// Snapshot sources (traceability)
// ---------------------------------------------------------------------------

define_repo_test!(snapshot_sources_traceability, |repos, _pool| async move {
    let team = repos
        .org
        .create_team("T", "O", TeamType::Team, None, None)
        .await
        .unwrap();

    let start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let end = time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap();
    let snap = SnapshotInput {
        id: Uuid::now_v7(),
        team_id: team.id,
        period_start: start,
        period_end: end,
        period_type: PeriodType::Week,
        throughput: 5,
        avg_review_turnaround_hours: None,
        avg_cycle_time_hours: None,
        wip_avg: None,
        flow_efficiency: None,
        lead_time_hours: None,
        raw_metrics: serde_json::json!({}),
    };
    let snap_id = repos.metrics.upsert_snapshot(&snap).await.unwrap();

    // Insert a contribution to link
    let item = make_contribution_at(
        Platform::Github,
        ContributionType::PullRequest,
        "trace-1",
        OffsetDateTime::now_utc(),
    );
    let contrib_id = Uuid::now_v7();
    repos
        .activity
        .upsert_contribution(contrib_id, None, &item)
        .await
        .unwrap();

    repos
        .metrics
        .insert_snapshot_sources(snap_id, &[contrib_id])
        .await
        .unwrap();

    // Verify we don't error on re-insert (ON CONFLICT DO NOTHING)
    repos
        .metrics
        .insert_snapshot_sources(snap_id, &[contrib_id])
        .await
        .unwrap();

    // Delete and verify
    repos
        .metrics
        .delete_snapshot_sources(snap_id)
        .await
        .unwrap();

    // Empty insert is a no-op
    repos
        .metrics
        .insert_snapshot_sources(snap_id, &[])
        .await
        .unwrap();
});
