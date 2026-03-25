use ps_proto::canonical::prism::v1::{GetCostSummaryRequest, GetCostSummaryResponse};
use tonic::{Request, Response, Status};

use super::super::common::{ai_provider_to_proto, db_err, require_auth};
use super::ReasoningServiceImpl;
use super::ai_settings::load_ai_config;

pub async fn get_cost_summary(
    svc: &ReasoningServiceImpl,
    request: Request<GetCostSummaryRequest>,
) -> Result<Response<GetCostSummaryResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let days = if req.days > 0 { req.days } else { 7 };

    let today = time::OffsetDateTime::now_utc().date();
    let since = today - time::Duration::days(i64::from(days) - 1);

    let (today_spend, daily_series, task_breakdown, model_breakdown, config) = tokio::try_join!(
        async {
            svc.repos
                .reasoning
                .get_daily_spend(today)
                .await
                .map_err(db_err)
        },
        async {
            svc.repos
                .reasoning
                .get_daily_spend_series(since, today)
                .await
                .map_err(db_err)
        },
        async {
            svc.repos
                .reasoning
                .get_daily_spend_by_task(today)
                .await
                .map_err(db_err)
        },
        async {
            let since_dt = since.midnight().assume_utc();
            let until_dt = (today + time::Duration::days(1)).midnight().assume_utc();
            svc.repos
                .reasoning
                .get_spend_summary(since_dt, until_dt)
                .await
                .map_err(db_err)
        },
        async { load_ai_config(svc).await },
    )?;

    let daily_spend: Vec<ps_proto::canonical::prism::v1::DailySpend> = daily_series
        .into_iter()
        .map(|d| ps_proto::canonical::prism::v1::DailySpend {
            date: d.date.to_string(),
            cost_usd: d.total_cost_usd,
            request_count: d.request_count,
        })
        .collect();

    let task_breakdown: Vec<ps_proto::canonical::prism::v1::TaskSpend> = task_breakdown
        .into_iter()
        .map(|t| ps_proto::canonical::prism::v1::TaskSpend {
            task_type: t.task_type,
            cost_usd: t.total_cost_usd,
            prompt_tokens: t.total_prompt_tokens,
            completion_tokens: t.total_completion_tokens,
            request_count: t.request_count,
        })
        .collect();

    let model_breakdown: Vec<ps_proto::canonical::prism::v1::ModelSpend> = model_breakdown
        .into_iter()
        .map(|m| ps_proto::canonical::prism::v1::ModelSpend {
            provider: ai_provider_to_proto(&m.provider),
            model: m.model,
            task_type: m.task_type,
            cost_usd: m.total_cost_usd,
            prompt_tokens: m.total_prompt_tokens,
            completion_tokens: m.total_completion_tokens,
            request_count: m.request_count,
        })
        .collect();

    Ok(Response::new(GetCostSummaryResponse {
        today_spend_usd: today_spend,
        budget_cap_usd: config.budget_cap_usd,
        daily_spend,
        task_breakdown,
        model_breakdown,
    }))
}
