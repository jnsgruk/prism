use crate::common::db::RepoTestContext;
use ps_core::ingestion::ContributionInput;
use ps_core::models::{ContributionState, ContributionType, Platform, TeamType};
use time::OffsetDateTime;
use uuid::Uuid;

/// Insert a person directly into org.people, returning the new ID.
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
    metrics: serde_json::Value,
    metadata: serde_json::Value,
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
        metrics,
        metadata,
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
    state_history: Option<serde_json::Value>,
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
        state_history,
        enrichment_content: None,
    }
}

/// Helper to seed contributions for a person, assign them to a team,
/// and then compute the snapshot. Returns the team_id.
async fn seed_team_with_contributions(
    repos: &ps_core::repo::Repos,
    pool: &sqlx::PgPool,
    contributions: &[ContributionInput],
) -> (Uuid, Uuid) {
    let team = repos
        .org
        .create_team("TestTeam", "TestOrg", TeamType::Team, None, None)
        .await
        .unwrap();
    let person = insert_person(pool, "Alice").await;

    // Insert contributions before assigning team (so start_date picks up earliest)
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

    (team.id, person)
}

// ---------------------------------------------------------------------------
// Cycle time from Jira tickets (end-to-end through DB)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cycle_time_from_jira_tickets() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let jan_15 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 15).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        make_jira_ticket(
            "JIRA-1",
            ContributionState::Closed,
            jan_10,
            Some(jan_15),
            Some(24.0),
            None,
        ),
        make_jira_ticket(
            "JIRA-2",
            ContributionState::Closed,
            jan_10,
            Some(jan_15),
            Some(48.0),
            None,
        ),
    ];

    let (team_id, _) = seed_team_with_contributions(repos, pool, &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await
        .unwrap();

    let cycle_time = ps_metrics::compute_cross_source_throughput(&contribs);
    assert_eq!(cycle_time.total, 2); // Both closed tickets count

    let ct = ps_metrics::flow::compute_cycle_time(&contribs).unwrap();
    assert!((ct - 36.0).abs() < 0.01); // (24 + 48) / 2

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Lead time from PRs and Jira
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lead_time_from_prs_and_jira() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(0, 0, 0).unwrap(),
    );
    let jan_11 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 11).unwrap(),
        time::Time::from_hms(0, 0, 0).unwrap(),
    );
    let jan_15 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 15).unwrap(),
        time::Time::from_hms(0, 0, 0).unwrap(),
    );

    let items = vec![
        // Merged PR: 24h lead time (jan_10 to jan_11)
        make_pr(
            "PR-1",
            ContributionState::Merged,
            jan_10,
            Some(jan_11),
            serde_json::json!({}),
            serde_json::json!({}),
        ),
        // Closed Jira: 48h cycle time
        make_jira_ticket(
            "JIRA-1",
            ContributionState::Closed,
            jan_10,
            Some(jan_15),
            Some(48.0),
            None,
        ),
    ];

    let (team_id, _) = seed_team_with_contributions(repos, pool, &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await
        .unwrap();

    let lead_time = ps_metrics::flow::compute_lead_time(&contribs).unwrap();
    assert!((lead_time - 36.0).abs() < 0.01); // (24 + 48) / 2

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// WIP counts open items
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wip_counts_open_items() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        // In-progress Jira ticket (no closed_at)
        make_jira_ticket(
            "JIRA-WIP-1",
            ContributionState::InProgress,
            jan_10,
            None,
            None,
            None,
        ),
        // Open PR (no closed_at)
        make_pr(
            "PR-WIP-1",
            ContributionState::Open,
            jan_10,
            None,
            serde_json::json!({}),
            serde_json::json!({}),
        ),
        // Closed ticket — should NOT count as WIP
        make_jira_ticket(
            "JIRA-DONE-1",
            ContributionState::Closed,
            jan_10,
            Some(jan_10),
            Some(24.0),
            None,
        ),
    ];

    let (team_id, _) = seed_team_with_contributions(repos, pool, &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await
        .unwrap();

    let wip = ps_metrics::flow::compute_wip(&contribs, period_end).unwrap();
    assert!((wip - 2.0).abs() < 0.01);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Throughput counts completed by platform
// ---------------------------------------------------------------------------

#[tokio::test]
async fn throughput_counts_completed_by_period() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let jan_12 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 12).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        make_pr(
            "PR-T-1",
            ContributionState::Merged,
            jan_10,
            Some(jan_12),
            serde_json::json!({}),
            serde_json::json!({}),
        ),
        make_pr(
            "PR-T-2",
            ContributionState::Open,
            jan_10,
            None,
            serde_json::json!({}),
            serde_json::json!({}),
        ), // not counted
        make_jira_ticket(
            "JIRA-T-1",
            ContributionState::Closed,
            jan_10,
            Some(jan_12),
            Some(10.0),
            None,
        ),
    ];

    let (team_id, _) = seed_team_with_contributions(repos, pool, &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await
        .unwrap();

    let throughput = ps_metrics::compute_cross_source_throughput(&contribs);
    assert_eq!(throughput.total, 2); // 1 merged PR + 1 closed Jira
    assert_eq!(throughput.by_source["github"], 1);
    assert_eq!(throughput.by_source["jira"], 1);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Flow efficiency from state durations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flow_efficiency_from_state_durations() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(0, 0, 0).unwrap(),
    );
    let jan_20 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 20).unwrap(),
        time::Time::from_hms(0, 0, 0).unwrap(),
    );

    // Total cycle time: 100 hours
    // Active time (In Progress → Done): 60 hours
    // Efficiency: 0.6
    let history = serde_json::json!([
        {"state": "To Do", "at": "2025-01-10T00:00:00Z"},
        {"state": "In Progress", "at": "2025-01-12T16:00:00Z"},
        {"state": "Done", "at": "2025-01-15T04:00:00Z"}
    ]);

    let items = vec![make_jira_ticket(
        "JIRA-EFF-1",
        ContributionState::Closed,
        jan_10,
        Some(jan_20),
        Some(100.0),
        Some(history),
    )];

    let (team_id, _) = seed_team_with_contributions(repos, pool, &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await
        .unwrap();

    let efficiency = ps_metrics::flow::compute_flow_efficiency(&contribs).unwrap();
    assert!((efficiency - 0.6).abs() < 0.01);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Empty period returns None
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_period_returns_none() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let team = repos
        .org
        .create_team("EmptyTeam", "Org", TeamType::Team, None, None)
        .await
        .unwrap();

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team.id, period_start, period_end)
        .await
        .unwrap();

    assert!(contribs.is_empty());
    assert_eq!(
        ps_metrics::compute_cross_source_throughput(&contribs).total,
        0
    );
    assert!(ps_metrics::flow::compute_cycle_time(&contribs).is_none());
    assert!(ps_metrics::flow::compute_wip(&contribs, period_end).is_none());
    assert!(ps_metrics::flow::compute_lead_time(&contribs).is_none());
    assert!(ps_metrics::flow::compute_flow_efficiency(&contribs).is_none());

    ctx.teardown().await;
}
