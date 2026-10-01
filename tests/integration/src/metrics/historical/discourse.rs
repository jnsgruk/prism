use super::*;

#[tokio::test]
async fn historical_topic_initial_post_and_reply_likes_are_both_counted_after_metric_change() {
    let ctx = RepoTestContext::new().await;
    let platform = Platform::Discourse("ubuntu".into());
    let person = create_person_with_identity(&ctx.pool, "Selected", &platform, "selected").await;
    let team = team(&ctx, "Team", None).await;
    membership(&ctx, person, team, date!(2020 - 01 - 01), None).await;
    let created = datetime!(2025-05-12 12:00 UTC);
    let mut topic = contribution("topic", created);
    topic.platform = platform.clone();
    topic.contribution_type = ContributionType::DiscourseTopic;
    topic.state = None;
    topic.metrics = serde_json::json!({"likes":3,"solved":true});
    let mut post = topic.clone();
    post.platform_id = "reply".into();
    post.contribution_type = ContributionType::DiscoursePost;
    post.metrics = serde_json::json!({"likes":2,"is_reply":true});
    let topic_id = Uuid::now_v7();
    let post_id = Uuid::now_v7();
    ctx.repos
        .activity
        .upsert_contribution(topic_id, Some(person), &topic)
        .await
        .unwrap();
    ctx.repos
        .activity
        .upsert_contribution(post_id, Some(person), &post)
        .await
        .unwrap();
    let owner = pipeline(&ctx).await;
    invalidate(&ctx, owner, topic_id, None, person, &[created.date()]).await;
    invalidate(&ctx, owner, post_id, None, person, &[created.date()]).await;
    drain(&ctx.repos, Some(owner), false).await;
    for kind in [PeriodType::Week, PeriodType::Month, PeriodType::Quarter] {
        let (start, _) = period_boundaries(created.date(), kind);
        let snapshot = ctx
            .repos
            .metrics
            .get_team_snapshot(team, start, kind)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.raw_metrics["discourse_likes_received"], 5);
        assert_eq!(snapshot.raw_metrics["discourse_topics_created"], 1);
        assert_eq!(snapshot.raw_metrics["discourse_posts"], 1);
        assert_eq!(snapshot.raw_metrics["discourse_replies"], 1);
    }
    topic.metrics["likes"] = 7.into();
    ctx.repos
        .activity
        .upsert_contribution(Uuid::now_v7(), Some(person), &topic)
        .await
        .unwrap();
    invalidate(
        &ctx,
        owner,
        topic_id,
        Some(person),
        person,
        &[created.date()],
    )
    .await;
    drain(&ctx.repos, Some(owner), false).await;
    for kind in [PeriodType::Week, PeriodType::Month, PeriodType::Quarter] {
        let (start, _) = period_boundaries(created.date(), kind);
        let snapshot = ctx
            .repos
            .metrics
            .get_team_snapshot(team, start, kind)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.raw_metrics["discourse_likes_received"], 9);
    }
    ctx.teardown().await;
}
