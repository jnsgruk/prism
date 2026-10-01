//! Historical refresh uses real repositories and seeded durable change fixtures.

use ps_core::{
    ingestion::ContributionInput,
    models::{
        ContributionState, ContributionType, EnrichmentType, PeriodType, Platform, TeamType,
        period_boundaries,
    },
    repo::{Repos, reasoning::UpsertEnrichmentParams},
};
use time::{
    Date, OffsetDateTime,
    macros::{date, datetime},
};
use uuid::Uuid;

use crate::common::{db::RepoTestContext, fixtures::create_person_with_identity};

fn contribution(key: &str, created_at: OffsetDateTime) -> ContributionInput {
    ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PullRequest,
        platform_id: key.into(),
        platform_username: "selected".into(),
        title: Some(key.into()),
        url: None,
        state: Some(ContributionState::Merged),
        created_at,
        updated_at: None,
        closed_at: Some(created_at),
        metrics: serde_json::json!({"additions":100,"deletions":10}),
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }
}

async fn team(ctx: &RepoTestContext, name: &str, parent: Option<Uuid>) -> Uuid {
    ctx.repos
        .org
        .create_team(name, "Org", TeamType::Team, parent, None)
        .await
        .unwrap()
        .id
}

async fn membership(
    ctx: &RepoTestContext,
    person: Uuid,
    team: Uuid,
    start: Date,
    end: Option<Date>,
) {
    let id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO org.team_memberships (id, person_id, team_id, start_date, end_date) VALUES ($1,$2,$3,$4,$5)",
        id,
        person, team, start, end,
    ).execute(&ctx.pool).await.unwrap();
}

async fn pipeline(ctx: &RepoTestContext) -> Uuid {
    let id = Uuid::now_v7();
    ctx.repos.activity.create_pipeline(id, None).await.unwrap();
    id
}

async fn invalidate(
    ctx: &RepoTestContext,
    pipeline: Uuid,
    contribution: Uuid,
    previous: Option<Uuid>,
    current: Uuid,
    dates: &[Date],
) {
    let mut periods = Vec::new();
    for reference in dates {
        for kind in [PeriodType::Week, PeriodType::Month, PeriodType::Quarter] {
            let (start, _) = period_boundaries(*reference, kind);
            let entry =
                serde_json::json!({"period_type":kind.as_str(),"period_start":start.to_string()});
            if !periods.contains(&entry) {
                periods.push(entry);
            }
        }
    }
    let periods = serde_json::json!(periods);
    let id = Uuid::now_v7();
    let source = Uuid::now_v7();
    sqlx::query!(
        r#"INSERT INTO activity.contribution_changes (id,pipeline_id,source_id,contribution_id,
        previous_person_id,current_person_id,current_created_at,current_input,affected_periods,input_hash)
        VALUES ($1,$2,$3,$4,$5,$6,now(),'{}',$7,$8)"#,
        id,pipeline,source,contribution,previous,current,periods,id.to_string(),
    ).execute(&ctx.pool).await.unwrap();
    sqlx::query!(
        r#"INSERT INTO activity.snapshot_invalidations (change_id,period_type,period_start)
        SELECT $1,period->>'period_type',(period->>'period_start')::date
        FROM jsonb_array_elements($2::jsonb) period"#,
        id,
        periods,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
}

async fn drain(repos: &Repos, owner: Option<Uuid>, insights: bool) -> usize {
    let mut count = 0;
    loop {
        let work = repos
            .activity
            .pending_snapshot_invalidations(owner, insights, 8)
            .await
            .unwrap();
        if work.is_empty() {
            return count;
        }
        for period in work {
            let (start, end) = period_boundaries(period.period_start, period.period_type);
            if insights {
                ps_reasoning::features::insights::compute_all_snapshots(
                    repos,
                    start,
                    end,
                    period.period_type,
                )
                .await
                .unwrap();
            } else {
                ps_metrics::compute_all_snapshots(repos, start, end, period.period_type)
                    .await
                    .unwrap();
            }
            repos
                .activity
                .acknowledge_snapshot_invalidations(&period.invalidation_ids, insights)
                .await
                .unwrap();
            count += 1;
        }
    }
}

async fn assert_snapshot(
    ctx: &RepoTestContext,
    team: Uuid,
    reference: Date,
    count: i32,
    sources: &[Uuid],
) {
    for kind in [PeriodType::Week, PeriodType::Month, PeriodType::Quarter] {
        let (start, _) = period_boundaries(reference, kind);
        let snapshot = ctx
            .repos
            .metrics
            .get_team_snapshot(team, start, kind)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.throughput, Some(count), "{kind} {start}");
        let mut actual = sqlx::query_scalar!(
            "SELECT contribution_id FROM metrics.snapshot_sources WHERE snapshot_id=$1 ORDER BY contribution_id",
            snapshot.id,
        ).fetch_all(&ctx.pool).await.unwrap();
        let mut expected = sources.to_vec();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected, "{kind} {start}");
    }
}

mod discourse;
mod insights;
mod raw;
mod recovery;
