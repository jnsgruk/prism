//! Bounded historical recomputation shared by owned processing and recovery.

use ps_core::repo::{Repos, activity::SnapshotInvalidationPeriod};
use restate_sdk::prelude::*;
use uuid::Uuid;

use crate::features::pipeline::ownership::OwnedProcessingRequest;
use crate::infra::run_lifecycle::{
    complete_owned_run, create_owned_run, ensure_owned_active, journaled_value,
    register_owned_invocation, register_owned_self, terminal_err,
};

pub(crate) async fn compute_owned_history(
    ctx: &Context<'_>,
    repos: &Repos,
    owner: &OwnedProcessingRequest,
    insights: bool,
) -> Result<(), TerminalError> {
    owner.validate_supported()?;
    ensure_owned_active!(ctx, repos, owner.pipeline_id)?;
    let handler_name = if insights {
        "InsightsHandler"
    } else {
        "MetricsComputeHandler"
    };
    let run_id = create_owned_run!(
        ctx,
        repos,
        owner.pipeline_id,
        "_system",
        handler_name,
        "compute_historical"
    );

    let mut computed = 0;
    let mut periods_completed = 0;
    loop {
        ensure_owned_active!(ctx, repos, owner.pipeline_id)?;
        let invocation = ctx
            .service_client::<HistoricalSnapshotServiceClient>()
            .refresh_batch(Json(HistoricalBatchRequest {
                owner: owner.clone(),
                parent_run_id: run_id,
                insights,
            }))
            .call();
        let handle = invocation.invocation_handle().await?;
        register_owned_invocation!(
            ctx,
            repos,
            owner.pipeline_id,
            handle,
            "historical_snapshot_batch",
            Some(run_id)
        );
        let result = invocation.await?.into_inner();
        computed += result.snapshots;
        periods_completed += result.periods;
        if result.periods == 0 {
            break;
        }

        let progress = serde_json::json!({
            "phase": "historical_refresh", "periods_completed": periods_completed,
            "snapshots_computed": computed, "insights": insights,
        });
        if let Err(error) = repos
            .activity
            .update_run_progress_detail(run_id, computed, &progress)
            .await
        {
            tracing::warn!(%run_id, %error, "failed to update historical refresh progress");
        }
    }
    complete_owned_run!(ctx, repos, run_id, "_system", computed);
    Ok(())
}

/// Database/computation failures stay retryable inside the named Restate step.
/// A cancellation after ingestion leaves the exact dirty generations available
/// to the recovery object once its owner reaches a terminal state.
macro_rules! recompute_period {
    ($ctx:expr, $repos:expr, $work:expr, $insights:expr, $owner:expr) => {{
        let work = $work;
        let name = format!(
            "refresh_{}_{}_{}",
            if $insights { "insights" } else { "metrics" },
            work.period_type,
            work.period_start
        );
        let repos = $repos.clone();
        let insights = $insights;
        let owner = $owner;
        $ctx.run(move || {
            $crate::features::metrics::historical::recompute_period_inner(
                repos.clone(),
                work.clone(),
                insights,
                owner,
            )
        })
        .name(name)
        .await?
        .into_inner()
    }};
}
pub(crate) use recompute_period;

pub(crate) async fn recompute_period_inner(
    repos: Repos,
    work: SnapshotInvalidationPeriod,
    insights: bool,
    owner: Option<Uuid>,
) -> Result<Json<i32>, HandlerError> {
    if let Some(pipeline_id) = owner
        && repos
            .activity
            .pipeline_cancel_requested(pipeline_id)
            .await?
    {
        return Err(TerminalError::new("pipeline cancelled").into());
    }
    let (start, end) = ps_core::models::period_boundaries(work.period_start, work.period_type);
    let count = if insights {
        ps_reasoning::features::insights::compute_all_snapshots(
            &repos,
            start,
            end,
            work.period_type,
        )
        .await?
    } else {
        ps_metrics::compute_all_snapshots(&repos, start, end, work.period_type).await?
    };
    if let Some(pipeline_id) = owner
        && repos
            .activity
            .pipeline_cancel_requested(pipeline_id)
            .await?
    {
        return Err(TerminalError::new("pipeline cancelled").into());
    }
    repos
        .activity
        .acknowledge_snapshot_invalidations(&work.invalidation_ids, insights)
        .await?;
    Ok(Json(count))
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoricalBatchRequest {
    pub owner: OwnedProcessingRequest,
    pub parent_run_id: Uuid,
    pub insights: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct HistoricalBatchResult {
    pub snapshots: i32,
    pub periods: usize,
}

pub struct HistoricalSnapshotServiceImpl {
    pub state: crate::infra::SharedState,
}

#[restate_sdk::service]
pub trait HistoricalSnapshotService {
    async fn refresh_batch(
        request: Json<HistoricalBatchRequest>,
    ) -> Result<Json<HistoricalBatchResult>, TerminalError>;
}

impl HistoricalSnapshotService for HistoricalSnapshotServiceImpl {
    async fn refresh_batch(
        &self,
        ctx: Context<'_>,
        Json(request): Json<HistoricalBatchRequest>,
    ) -> Result<Json<HistoricalBatchResult>, TerminalError> {
        let repos = &self.state.repos;
        let pipeline_id = request.owner.pipeline_id;
        request.owner.validate_supported()?;
        register_owned_self!(
            ctx,
            repos,
            pipeline_id,
            "historical_snapshot_batch",
            Some(request.parent_run_id)
        );
        ensure_owned_active!(ctx, repos, pipeline_id)?;
        let insights = request.insights;
        let work = journaled_value!(ctx, "historical_periods", [repos], {
            repos
                .activity
                .pending_snapshot_invalidations(Some(pipeline_id), insights, 8)
                .await?
        });
        let periods = work.len();
        let mut snapshots = 0;
        for period in work {
            ensure_owned_active!(ctx, repos, pipeline_id)?;
            snapshots += recompute_period!(ctx, repos, period, insights, Some(pipeline_id));
        }
        Ok(Json(HistoricalBatchResult { snapshots, periods }))
    }
}
