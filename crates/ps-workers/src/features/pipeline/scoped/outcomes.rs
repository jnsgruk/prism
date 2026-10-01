use ps_core::models::IngestionStatus;

use crate::features::pipeline::stages::{HandlerResult, StageStatus};

pub(super) fn apply_source_outcomes(
    results: &mut [HandlerResult],
    runs: &[(String, IngestionStatus, Option<i32>)],
) {
    for result in results {
        let Some((_, status, count)) = runs.iter().find(|(name, _, _)| name == &result.name) else {
            result.status = StageStatus::Failed;
            result.error = Some("Source completion could not be verified".into());
            continue;
        };
        result.items = *count;
        match status {
            IngestionStatus::Completed => {}
            IngestionStatus::CompletedWithWarnings => {
                result.status = StageStatus::CompletedWithWarnings;
                result.error = Some("Source coverage is incomplete; see source run details".into());
            }
            _ => {
                result.status = StageStatus::Failed;
                result.error = Some("Source ingestion failed; see source run details".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_source_return_cannot_claim_complete_coverage() {
        for initial_status in [StageStatus::Completed, StageStatus::Failed] {
            let mut result = [HandlerResult {
                name: "selected source".into(),
                status: initial_status,
                items: None,
                error: None,
            }];
            apply_source_outcomes(
                &mut result,
                &[(
                    "selected source".into(),
                    IngestionStatus::CompletedWithWarnings,
                    Some(2),
                )],
            );
            assert_eq!(result[0].status, StageStatus::CompletedWithWarnings);
            assert_eq!(result[0].items, Some(2));
            assert!(result[0].error.is_some());
        }
    }

    #[test]
    fn missing_and_failed_runs_are_fail_closed() {
        for runs in [
            vec![],
            vec![("selected source".into(), IngestionStatus::Failed, Some(0))],
        ] {
            let mut result = [HandlerResult {
                name: "selected source".into(),
                status: StageStatus::Completed,
                items: None,
                error: None,
            }];
            apply_source_outcomes(&mut result, &runs);
            assert_eq!(result[0].status, StageStatus::Failed);
        }
    }
}
