use ps_proto::canonical::prism::v1::{GetUsageSummaryRequest, GetUsageSummaryResponse};
use tonic::{Request, Response, Status};

use super::ReasoningServiceImpl;
use crate::common::{db_err, require_auth};

pub async fn get_usage_summary(
    svc: &ReasoningServiceImpl,
    request: Request<GetUsageSummaryRequest>,
) -> Result<Response<GetUsageSummaryResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let days = if req.days > 0 { req.days } else { 7 };

    let today = time::OffsetDateTime::now_utc().date();
    let since = today - time::Duration::days(i64::from(days) - 1);
    let since_dt = since.midnight().assume_utc();
    let until_dt = (today + time::Duration::days(1)).midnight().assume_utc();

    let (task_breakdown, model_breakdown) = tokio::try_join!(
        async {
            svc.repos
                .reasoning
                .get_usage_by_task(since_dt, until_dt)
                .await
                .map_err(db_err)
        },
        async {
            svc.repos
                .reasoning
                .get_usage_by_model(since_dt, until_dt)
                .await
                .map_err(db_err)
        },
    )?;

    let task_breakdown = task_breakdown
        .into_iter()
        .map(|t| ps_proto::canonical::prism::v1::TaskUsage {
            task_type: t.task_type,
            prompt_tokens: t.total_prompt_tokens,
            completion_tokens: t.total_completion_tokens,
            request_count: t.request_count,
        })
        .collect();

    let model_breakdown = model_breakdown
        .into_iter()
        .map(|m| ps_proto::canonical::prism::v1::ModelUsage {
            provider: m.provider,
            model: m.model,
            task_type: m.task_type,
            prompt_tokens: m.total_prompt_tokens,
            completion_tokens: m.total_completion_tokens,
            request_count: m.request_count,
        })
        .collect();

    Ok(Response::new(GetUsageSummaryResponse {
        task_breakdown,
        model_breakdown,
    }))
}
