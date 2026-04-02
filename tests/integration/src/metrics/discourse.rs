use crate::define_repo_test;
use ps_core::ingestion::ContributionInput;
use ps_core::models::{ContributionType, PeriodType, Platform, TeamType};
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

fn make_discourse(
    platform: Platform,
    contribution_type: ContributionType,
    platform_id: &str,
    created_at: OffsetDateTime,
    metrics: serde_json::Value,
) -> ContributionInput {
    ContributionInput {
        platform,
        contribution_type,
        platform_id: platform_id.into(),
        platform_username: "discourse_user".into(),
        title: Some(format!("Discourse {platform_id}")),
        url: None,
        state: None,
        created_at,
        updated_at: None,
        closed_at: None,
        metrics,
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }
}

/// Seed a team with one person and their contributions.
async fn seed_team(
    repos: &ps_core::repo::Repos,
    pool: &sqlx::PgPool,
    team_name: &str,
    person_name: &str,
    contributions: &[ContributionInput],
) -> (Uuid, Uuid) {
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

    (team.id, person)
}

// ---------------------------------------------------------------------------
// Basic Discourse metrics through real DB
// ---------------------------------------------------------------------------

define_repo_test!(basic_discourse_metrics, |repos, pool| async move {
    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let ubuntu = Platform::Discourse("ubuntu".into());

    let items = vec![
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscourseTopic,
            "topic-1",
            jan_10,
            serde_json::json!({"solved": true, "views": 100}),
        ),
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscourseTopic,
            "topic-2",
            jan_10,
            serde_json::json!({"solved": false}),
        ),
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscoursePost,
            "post-1",
            jan_10,
            serde_json::json!({"is_reply": true, "likes": 3}),
        ),
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscoursePost,
            "post-2",
            jan_10,
            serde_json::json!({"is_reply": false, "likes": 1}),
        ),
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscourseLike,
            "like-1",
            jan_10,
            serde_json::json!({}),
        ),
    ];

    let (team_id, _) = seed_team(&repos, &pool, "DiscourseTeam", "Alice", &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await
        .unwrap();

    let discourse = ps_metrics::compute_discourse_metrics(&contribs).unwrap();
    assert_eq!(discourse.topics_created, 2);
    assert_eq!(discourse.posts, 2);
    assert_eq!(discourse.replies, 1);
    assert_eq!(discourse.likes_given, 1);
    assert_eq!(discourse.likes_received, 4);
    assert_eq!(discourse.solved_topics, 1);
    assert_eq!(discourse.active_participants, 1); // single person

    let inst = &discourse.by_instance["ubuntu"];
    assert_eq!(inst.topics_created, 2);
    assert_eq!(inst.posts, 2);
    assert_eq!(inst.likes_given, 1);
});

// ---------------------------------------------------------------------------
// Multi-instance breakdown
// ---------------------------------------------------------------------------

define_repo_test!(multi_instance_breakdown, |repos, pool| async move {
    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );

    let items = vec![
        make_discourse(
            Platform::Discourse("ubuntu".into()),
            ContributionType::DiscourseTopic,
            "ubuntu-topic-1",
            jan_10,
            serde_json::json!({}),
        ),
        make_discourse(
            Platform::Discourse("snapcraft".into()),
            ContributionType::DiscoursePost,
            "snap-post-1",
            jan_10,
            serde_json::json!({"is_reply": true, "likes": 0}),
        ),
        make_discourse(
            Platform::Discourse("snapcraft".into()),
            ContributionType::DiscourseTopic,
            "snap-topic-1",
            jan_10,
            serde_json::json!({"solved": true}),
        ),
    ];

    let (team_id, _) = seed_team(&repos, &pool, "MultiInstance", "Bob", &items).await;

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await
        .unwrap();

    let discourse = ps_metrics::compute_discourse_metrics(&contribs).unwrap();
    assert_eq!(discourse.topics_created, 2);
    assert_eq!(discourse.posts, 1);
    assert_eq!(discourse.solved_topics, 1);
    assert_eq!(discourse.by_instance.len(), 2);
    assert_eq!(discourse.by_instance["ubuntu"].topics_created, 1);
    assert_eq!(discourse.by_instance["snapcraft"].topics_created, 1);
    assert_eq!(discourse.by_instance["snapcraft"].posts, 1);
});

// ---------------------------------------------------------------------------
// Discourse metrics per-person (multiple team members)
// ---------------------------------------------------------------------------

define_repo_test!(discourse_metrics_per_person, |repos, pool| async move {
    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let ubuntu = Platform::Discourse("ubuntu".into());

    let team = repos
        .org
        .create_team("PerPersonTeam", "TestOrg", TeamType::Team, None, None)
        .await
        .unwrap();

    // Person A: 2 topics
    let alice = insert_person(&pool, "Alice").await;
    for (i, item) in [
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscourseTopic,
            "alice-topic-1",
            jan_10,
            serde_json::json!({}),
        ),
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscourseTopic,
            "alice-topic-2",
            jan_10,
            serde_json::json!({}),
        ),
    ]
    .iter()
    .enumerate()
    {
        let _ = i;
        repos
            .activity
            .upsert_contribution(Uuid::now_v7(), Some(alice), item)
            .await
            .unwrap();
    }
    repos
        .org
        .assign_person_to_team(alice.into(), team.id.into())
        .await
        .unwrap();

    // Person B: 1 topic + 1 post
    let bob = insert_person(&pool, "Bob").await;
    for item in &[
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscourseTopic,
            "bob-topic-1",
            jan_10,
            serde_json::json!({}),
        ),
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscoursePost,
            "bob-post-1",
            jan_10,
            serde_json::json!({"is_reply": true, "likes": 5}),
        ),
    ] {
        repos
            .activity
            .upsert_contribution(Uuid::now_v7(), Some(bob), item)
            .await
            .unwrap();
    }
    repos
        .org
        .assign_person_to_team(bob.into(), team.id.into())
        .await
        .unwrap();

    let period_start = time::Date::from_calendar_date(2025, time::Month::January, 6).unwrap();
    let period_end = time::Date::from_calendar_date(2025, time::Month::January, 31).unwrap();

    let contribs = repos
        .metrics
        .get_team_contributions(team.id, period_start, period_end)
        .await
        .unwrap();

    let discourse = ps_metrics::compute_discourse_metrics(&contribs).unwrap();
    assert_eq!(discourse.topics_created, 3); // 2 Alice + 1 Bob
    assert_eq!(discourse.posts, 1);
    assert_eq!(discourse.replies, 1);
    assert_eq!(discourse.likes_received, 5);
    assert_eq!(discourse.active_participants, 2); // Alice + Bob
});

// ---------------------------------------------------------------------------
// Discourse snapshot end-to-end (compute_team_snapshot stores discourse data)
// ---------------------------------------------------------------------------

define_repo_test!(discourse_snapshot_stored, |repos, pool| async move {
    let jan_10 = OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2025, time::Month::January, 10).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    );
    let ubuntu = Platform::Discourse("ubuntu".into());

    let items = vec![
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscourseTopic,
            "snap-t-1",
            jan_10,
            serde_json::json!({"solved": true}),
        ),
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscoursePost,
            "snap-p-1",
            jan_10,
            serde_json::json!({"is_reply": true, "likes": 2}),
        ),
        make_discourse(
            ubuntu.clone(),
            ContributionType::DiscourseLike,
            "snap-l-1",
            jan_10,
            serde_json::json!({}),
        ),
    ];

    let (team_id, _) = seed_team(&repos, &pool, "DiscSnapTeam", "Carol", &items).await;

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
        .expect("discourse snapshot should exist");

    let raw: serde_json::Value = snap.raw_metrics;
    assert_eq!(raw["discourse_topics_created"], 1);
    assert_eq!(raw["discourse_posts"], 1);
    assert_eq!(raw["discourse_replies"], 1);
    assert_eq!(raw["discourse_likes_given"], 1);
    assert_eq!(raw["discourse_likes_received"], 2);
    assert_eq!(raw["discourse_solved_topics"], 1);
    assert_eq!(raw["discourse_active_participants"], 1);

    // Per-instance breakdown
    let by_instance = &raw["discourse_by_instance"]["ubuntu"];
    assert_eq!(by_instance["topics_created"], 1);
    assert_eq!(by_instance["posts"], 1);
    assert_eq!(by_instance["likes_given"], 1);
});
