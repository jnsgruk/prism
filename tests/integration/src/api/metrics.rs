use crate::define_api_test;
use ps_core::ingestion::ContributionInput;
use ps_core::models::{ContributionState, ContributionType, Platform, TeamType};
use ps_proto::canonical::prism::v1::metrics_service_client::MetricsServiceClient;
use ps_proto::canonical::prism::v1::{
    ContributionType as ProtoContributionType, GetContributionRequest, GetFlowMetricsRequest,
    GetIndividualProfileRequest, GetTeamMetricsRequest, ListPeriodsRequest,
    ListTeamContributionsRequest, Period, PeriodType, Platform as ProtoPlatform,
};
use time::OffsetDateTime;
use tonic::Request;
use tonic::metadata::MetadataValue;
use uuid::Uuid;

fn auth<T>(req: &mut Request<T>, token: &str) {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
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
        url: Some("https://github.com/org/repo/pull/1".into()),
        state: Some(state),
        created_at,
        updated_at: None,
        closed_at,
        metrics: serde_json::json!({"cycle_time_hours": 24.0}),
        metadata: serde_json::json!({"additions": 10, "deletions": 5}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }
}

/// Helper: insert a person, create a team, assign them, seed contributions.
/// Returns (team_id, person_id).
async fn seed_team(
    repos: &ps_core::repo::Repos,
    pool: &sqlx::PgPool,
    contributions: &[ContributionInput],
) -> (Uuid, Uuid) {
    let team = repos
        .org
        .create_team("TestTeam", "TestOrg", TeamType::Team, None, None)
        .await
        .unwrap();

    let person_id = Uuid::now_v7();
    sqlx::query("INSERT INTO org.people (id, name) VALUES ($1, $2)")
        .bind(person_id)
        .bind("Alice")
        .execute(pool)
        .await
        .unwrap();

    // Create platform identity so contribution resolution works
    let identity_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO org.platform_identities (id, person_id, platform, platform_username) VALUES ($1, $2, $3, $4)",
    )
    .bind(identity_id)
    .bind(person_id)
    .bind("github")
    .bind("testuser")
    .execute(pool)
    .await
    .unwrap();

    for item in contributions {
        repos
            .activity
            .upsert_contribution(Uuid::now_v7(), Some(person_id), item)
            .await
            .unwrap();
    }

    repos
        .org
        .assign_person_to_team(person_id, team.id)
        .await
        .unwrap();

    (team.id, person_id)
}

fn past_period() -> Period {
    Period {
        r#type: PeriodType::Week.into(),
        start: "2025-01-06".into(),
        end: "2025-01-12".into(),
    }
}

// ---------------------------------------------------------------------------
// GetTeamMetrics — empty
// ---------------------------------------------------------------------------

define_api_test!(get_team_metrics_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let team = repos
        .org
        .create_team("EmptyTeam", "Org", TeamType::Team, None, None)
        .await
        .unwrap();

    let mut req = Request::new(GetTeamMetricsRequest {
        team_id: team.id.to_string(),
        period: Some(past_period()),
    });
    auth(&mut req, &token);

    let resp = client
        .get_team_metrics(req)
        .await
        .expect("get_team_metrics")
        .into_inner();

    let metrics = resp.metrics.expect("should have metrics");
    assert_eq!(metrics.team_name, "EmptyTeam");
    assert_eq!(metrics.throughput, 0);
});

// ---------------------------------------------------------------------------
// GetTeamMetrics — with seeded data
// ---------------------------------------------------------------------------

define_api_test!(get_team_metrics_with_data, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        make_pr("PR-1", ContributionState::Merged, jan_10, Some(jan_10)),
        make_pr("PR-2", ContributionState::Merged, jan_10, Some(jan_10)),
    ];

    let (team_id, _) = seed_team(&repos, &server.pool, &items).await;

    let mut req = Request::new(GetTeamMetricsRequest {
        team_id: team_id.to_string(),
        period: Some(past_period()),
    });
    auth(&mut req, &token);

    let resp = client
        .get_team_metrics(req)
        .await
        .expect("get_team_metrics")
        .into_inner();

    let metrics = resp.metrics.expect("should have metrics");
    assert_eq!(metrics.throughput, 2);
});

// ---------------------------------------------------------------------------
// ListTeamContributions — paginated
// ---------------------------------------------------------------------------

define_api_test!(list_team_contributions, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items: Vec<ContributionInput> = (0..3)
        .map(|i| {
            make_pr(
                &format!("PR-{i}"),
                ContributionState::Merged,
                jan_10,
                Some(jan_10),
            )
        })
        .collect();

    let (team_id, _) = seed_team(&repos, &server.pool, &items).await;

    let mut req = Request::new(ListTeamContributionsRequest {
        team_id: team_id.to_string(),
        period: Some(past_period()),
        contribution_type: 0, // UNSPECIFIED
        state: 0,             // UNSPECIFIED
        page_size: 2,
        page_index: 0,
        sort_field: None,
        sort_desc: None,
        search: None,
        platform: 0, // UNSPECIFIED
        platform_instance: None,
    });
    auth(&mut req, &token);

    let resp = client
        .list_team_contributions(req)
        .await
        .expect("list_team_contributions")
        .into_inner();

    assert_eq!(resp.contributions.len(), 2);
    assert_eq!(resp.total_count, 3);
});

// ---------------------------------------------------------------------------
// GetContribution — by ID
// ---------------------------------------------------------------------------

define_api_test!(get_contribution_by_id, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let item = make_pr("PR-GET-1", ContributionState::Merged, jan_10, Some(jan_10));
    let contribution_id = Uuid::now_v7();
    repos
        .activity
        .upsert_contribution(contribution_id, None, &item)
        .await
        .unwrap();

    let mut req = Request::new(GetContributionRequest {
        contribution_id: contribution_id.to_string(),
    });
    auth(&mut req, &token);

    let resp = client
        .get_contribution(req)
        .await
        .expect("get_contribution")
        .into_inner();

    let c = resp.contribution.expect("contribution present");
    assert_eq!(c.title, "PR PR-GET-1");
    assert_eq!(c.platform, i32::from(ProtoPlatform::Github));
    assert_eq!(
        c.contribution_type,
        i32::from(ProtoContributionType::PullRequest)
    );
});

// ---------------------------------------------------------------------------
// GetContribution — not found
// ---------------------------------------------------------------------------

define_api_test!(get_contribution_not_found, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetContributionRequest {
        contribution_id: Uuid::now_v7().to_string(),
    });
    auth(&mut req, &token);

    let err = client
        .get_contribution(req)
        .await
        .expect_err("should be not found");

    assert_eq!(err.code(), tonic::Code::NotFound);
});

// ---------------------------------------------------------------------------
// GetFlowMetrics — with seeded data
// ---------------------------------------------------------------------------

define_api_test!(get_flow_metrics_with_data, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        make_pr("PR-F1", ContributionState::Merged, jan_10, Some(jan_10)),
        make_pr("PR-F2", ContributionState::Merged, jan_10, Some(jan_10)),
    ];

    let (team_id, _) = seed_team(&repos, &server.pool, &items).await;

    let mut req = Request::new(GetFlowMetricsRequest {
        team_id: team_id.to_string(),
        period: Some(past_period()),
    });
    auth(&mut req, &token);

    let resp = client
        .get_flow_metrics(req)
        .await
        .expect("get_flow_metrics")
        .into_inner();

    assert_eq!(resp.throughput, 2);
});

// ---------------------------------------------------------------------------
// GetIndividualProfile — returns person data
// ---------------------------------------------------------------------------

define_api_test!(get_individual_profile, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![make_pr(
        "PR-IND-1",
        ContributionState::Merged,
        jan_10,
        Some(jan_10),
    )];

    let (_, person_id) = seed_team(&repos, &server.pool, &items).await;

    let mut req = Request::new(GetIndividualProfileRequest {
        person_id: person_id.to_string(),
        period: Some(past_period()),
    });
    auth(&mut req, &token);

    let resp = client
        .get_individual_profile(req)
        .await
        .expect("get_individual_profile")
        .into_inner();

    assert_eq!(resp.name, "Alice");
    assert!(!resp.identities.is_empty());
    assert_eq!(
        resp.identities[0].platform,
        i32::from(ProtoPlatform::Github)
    );
    assert_eq!(resp.identities[0].username, "testuser");
});

// ---------------------------------------------------------------------------
// ListPeriods — empty initially
// ---------------------------------------------------------------------------

define_api_test!(list_periods_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let mut req = Request::new(ListPeriodsRequest {});
    auth(&mut req, &token);

    let resp = client
        .list_periods(req)
        .await
        .expect("list_periods")
        .into_inner();

    assert!(resp.periods.is_empty());
});

// ---------------------------------------------------------------------------
// GetTeamMetrics — requires auth
// ---------------------------------------------------------------------------

define_api_test!(get_team_metrics_requires_auth, |server| async move {
    let mut client = MetricsServiceClient::new(server.channel.clone());

    let err = client
        .get_team_metrics(GetTeamMetricsRequest {
            team_id: Uuid::now_v7().to_string(),
            period: Some(past_period()),
        })
        .await
        .expect_err("should require auth");

    assert_eq!(err.code(), tonic::Code::Unauthenticated);
});
