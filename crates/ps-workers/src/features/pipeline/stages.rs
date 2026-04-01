use restate_sdk::prelude::TerminalError;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Status of a pipeline stage or handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
    Cancelled,
}

impl StageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Result of a single handler invocation within a stage.
#[derive(Debug, Clone)]
pub struct HandlerResult {
    pub name: String,
    pub status: StageStatus,
    pub items: Option<i32>,
    pub error: Option<String>,
}

/// Ordered list of all pipeline stages.
pub const STAGE_NAMES: &[&str] = &[
    "team_sync",
    "ingestion",
    "metrics",
    "enrichment",
    "embedding",
    "insights",
    "identity_resolution",
];

/// Which branch a stage belongs to (for the fork after ingestion).
pub fn stage_branch(stage: &str) -> &'static str {
    match stage {
        "identity_resolution" => "identity",
        "team_sync" | "ingestion" => "pre_fork",
        _ => "main",
    }
}

/// Build the initial stages JSONB with all stages pending.
pub fn build_initial_stages(
    has_github: bool,
    has_discourse: bool,
    handler_names: &[(&str, Vec<String>)],
) -> serde_json::Value {
    let mut stages = serde_json::Map::new();

    for &stage_name in STAGE_NAMES {
        // Skip conditional stages if not applicable
        if stage_name == "team_sync" && !has_github {
            continue;
        }
        if stage_name == "identity_resolution" && !has_discourse {
            continue;
        }

        let handlers: Vec<serde_json::Value> = handler_names
            .iter()
            .filter(|(s, _)| *s == stage_name)
            .flat_map(|(_, names)| names.iter())
            .map(|name| {
                json!({
                    "name": name,
                    "status": "pending"
                })
            })
            .collect();

        let mut stage = serde_json::Map::new();
        stage.insert("status".into(), json!("pending"));
        if let Some(branch) = match stage_branch(stage_name) {
            "pre_fork" => None,
            b => Some(b),
        } {
            stage.insert("branch".into(), json!(branch));
        }
        stage.insert("handlers".into(), json!(handlers));
        stages.insert(stage_name.into(), json!(stage));
    }

    serde_json::Value::Object(stages)
}

/// Update a stage to "running" in the stages JSONB.
///
/// NOTE: No timestamps here — `OffsetDateTime::now_utc()` is non-deterministic
/// and causes Restate error 570 on journal replay. Timestamps are added in the
/// DB persist layer instead (inside journaled blocks).
pub fn mark_stage_running(stages: &mut serde_json::Value, stage_name: &str) {
    if let Some(stage) = stages.get_mut(stage_name) {
        stage["status"] = json!("running");
    }
}

/// Update a stage with handler results and compute its overall status.
pub fn mark_stage_complete(
    stages: &mut serde_json::Value,
    stage_name: &str,
    results: &[HandlerResult],
) {
    let overall = derive_stage_status(results);

    if let Some(stage) = stages.get_mut(stage_name) {
        stage["status"] = json!(overall.as_str());

        let handler_json: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let mut h = serde_json::Map::new();
                h.insert("name".into(), json!(r.name));
                h.insert("status".into(), json!(r.status.as_str()));
                if let Some(items) = r.items {
                    h.insert("items".into(), json!(items));
                }
                if let Some(ref err) = r.error {
                    h.insert("error".into(), json!(err));
                }
                serde_json::Value::Object(h)
            })
            .collect();

        stage["handlers"] = json!(handler_json);
    }
}

/// Mark a stage as skipped.
pub fn mark_stage_skipped(stages: &mut serde_json::Value, stage_name: &str) {
    if let Some(stage) = stages.get_mut(stage_name) {
        stage["status"] = json!("skipped");
    }
}

/// Mark remaining pending stages as cancelled.
pub fn mark_remaining_cancelled(stages: &mut serde_json::Value) {
    if let Some(obj) = stages.as_object_mut() {
        for (_name, stage) in obj.iter_mut() {
            if stage.get("status").and_then(|s| s.as_str()) == Some("pending") {
                stage["status"] = json!("cancelled");
            }
        }
    }
}

/// Derive overall stage status from individual handler results.
pub fn derive_stage_status(results: &[HandlerResult]) -> StageStatus {
    if results.is_empty() {
        return StageStatus::Completed;
    }

    let all_failed = results.iter().all(|r| r.status == StageStatus::Failed);
    let any_failed = results.iter().any(|r| r.status == StageStatus::Failed);

    if all_failed {
        StageStatus::Failed
    } else if any_failed {
        // Partial failure: some succeeded, some failed
        StageStatus::Completed // completed_with_warnings at pipeline level
    } else {
        StageStatus::Completed
    }
}

/// Derive the final pipeline status from all stage statuses.
pub fn derive_pipeline_status(stages: &serde_json::Value) -> &'static str {
    let statuses: Vec<&str> = stages
        .as_object()
        .map(|obj| {
            obj.values()
                .filter_map(|stage| stage.get("status").and_then(|s| s.as_str()))
                .collect()
        })
        .unwrap_or_default();

    let any_failed = statuses.contains(&"failed");
    let any_cancelled = statuses.contains(&"cancelled");

    // Check for partial failures in handler results
    let any_handler_failed = stages.as_object().is_some_and(|obj| {
        obj.values().any(|stage| {
            stage
                .get("handlers")
                .and_then(|h| h.as_array())
                .is_some_and(|handlers| {
                    handlers
                        .iter()
                        .any(|h| h.get("status").and_then(|s| s.as_str()) == Some("failed"))
                })
        })
    });

    if any_cancelled {
        "cancelled"
    } else if any_failed {
        "failed"
    } else if any_handler_failed {
        "completed_with_warnings"
    } else {
        "completed"
    }
}

/// Result returned from the pipeline workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    pub pipeline_id: String,
    pub status: String,
}

/// Status response for `get_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatus {
    pub current_stage: Option<String>,
    pub stages: serde_json::Value,
}

/// Lightweight source info that can be journaled.
/// `source_type` is the `Platform::to_string()` value (e.g. `"github"`, `"discourse_ubuntu"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub name: String,
    pub source_type: String,
}

/// Convert a handler call result into a `HandlerResult`.
pub fn call_result(name: String, result: &Result<(), TerminalError>) -> HandlerResult {
    HandlerResult {
        name,
        status: if result.is_ok() {
            StageStatus::Completed
        } else {
            StageStatus::Failed
        },
        items: None,
        error: result.as_ref().err().map(ToString::to_string),
    }
}

/// Build the handler name list for initial stages JSONB.
pub fn build_handler_list(
    sources: &[SourceInfo],
    has_github: bool,
    has_discourse: bool,
) -> Vec<(&'static str, Vec<String>)> {
    let mut list = Vec::new();

    if has_github {
        let github_names: Vec<String> = sources
            .iter()
            .filter(|s| s.source_type == "github")
            .map(|s| format!("{} Team Sync", s.name))
            .collect();
        list.push(("team_sync", github_names));
    }

    let ingestion_names: Vec<String> = sources.iter().map(|s| s.name.clone()).collect();
    list.push(("ingestion", ingestion_names));

    list.push(("metrics", vec!["Metrics".into()]));
    list.push(("enrichment", vec!["Enrichment".into()]));
    list.push(("embedding", vec!["Embedding".into()]));
    list.push(("insights", vec!["Insights".into()]));

    if has_discourse {
        list.push(("identity_resolution", vec!["Identity Resolution".into()]));
    }

    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_stage_status_all_completed() {
        let results = vec![
            HandlerResult {
                name: "Github".into(),
                status: StageStatus::Completed,
                items: Some(10),
                error: None,
            },
            HandlerResult {
                name: "Jira".into(),
                status: StageStatus::Completed,
                items: Some(5),
                error: None,
            },
        ];
        assert_eq!(derive_stage_status(&results), StageStatus::Completed);
    }

    #[test]
    fn derive_stage_status_all_failed() {
        let results = vec![
            HandlerResult {
                name: "Github".into(),
                status: StageStatus::Failed,
                items: None,
                error: Some("auth error".into()),
            },
            HandlerResult {
                name: "Jira".into(),
                status: StageStatus::Failed,
                items: None,
                error: Some("timeout".into()),
            },
        ];
        assert_eq!(derive_stage_status(&results), StageStatus::Failed);
    }

    #[test]
    fn derive_stage_status_partial_failure() {
        let results = vec![
            HandlerResult {
                name: "Github".into(),
                status: StageStatus::Completed,
                items: Some(10),
                error: None,
            },
            HandlerResult {
                name: "Jira".into(),
                status: StageStatus::Failed,
                items: None,
                error: Some("auth error".into()),
            },
        ];
        // Partial failure → still Completed at stage level (warnings at pipeline level)
        assert_eq!(derive_stage_status(&results), StageStatus::Completed);
    }

    #[test]
    fn derive_stage_status_empty() {
        assert_eq!(derive_stage_status(&[]), StageStatus::Completed);
    }

    #[test]
    fn derive_pipeline_status_all_completed() {
        let stages = json!({
            "ingestion": { "status": "completed", "handlers": [{"name": "Github", "status": "completed"}] },
            "metrics": { "status": "completed", "handlers": [{"name": "Metrics", "status": "completed"}] }
        });
        assert_eq!(derive_pipeline_status(&stages), "completed");
    }

    #[test]
    fn derive_pipeline_status_with_handler_failure() {
        let stages = json!({
            "ingestion": {
                "status": "completed",
                "handlers": [
                    {"name": "Github", "status": "completed"},
                    {"name": "Jira", "status": "failed", "error": "auth"}
                ]
            },
            "metrics": { "status": "completed", "handlers": [{"name": "Metrics", "status": "completed"}] }
        });
        assert_eq!(derive_pipeline_status(&stages), "completed_with_warnings");
    }

    #[test]
    fn derive_pipeline_status_stage_failed() {
        let stages = json!({
            "ingestion": { "status": "failed", "handlers": [{"name": "Github", "status": "failed"}] },
            "metrics": { "status": "skipped", "handlers": [] }
        });
        assert_eq!(derive_pipeline_status(&stages), "failed");
    }

    #[test]
    fn derive_pipeline_status_cancelled() {
        let stages = json!({
            "ingestion": { "status": "completed", "handlers": [] },
            "metrics": { "status": "cancelled", "handlers": [] }
        });
        assert_eq!(derive_pipeline_status(&stages), "cancelled");
    }

    #[test]
    fn build_initial_stages_full() {
        let handler_names = vec![
            ("team_sync", vec!["Github Team Sync".into()]),
            (
                "ingestion",
                vec!["Github".into(), "Jira".into(), "Discourse".into()],
            ),
            ("metrics", vec!["Metrics".into()]),
            ("enrichment", vec!["Enrichment".into()]),
            ("embedding", vec!["Embedding".into()]),
            ("insights", vec!["Insights".into()]),
            ("identity_resolution", vec!["Identity Resolution".into()]),
        ];
        let stages = build_initial_stages(true, true, &handler_names);

        assert_eq!(stages["team_sync"]["status"], "pending");
        assert_eq!(stages["ingestion"]["handlers"].as_array().unwrap().len(), 3);
        assert_eq!(stages["identity_resolution"]["branch"], "identity");
        assert_eq!(stages["metrics"]["branch"], "main");
    }

    #[test]
    fn build_initial_stages_no_github_no_discourse() {
        let handler_names = vec![
            ("ingestion", vec!["Jira".into()]),
            ("metrics", vec!["Metrics".into()]),
            ("enrichment", vec!["Enrichment".into()]),
            ("embedding", vec!["Embedding".into()]),
            ("insights", vec!["Insights".into()]),
        ];
        let stages = build_initial_stages(false, false, &handler_names);

        assert!(stages.get("team_sync").is_none());
        assert!(stages.get("identity_resolution").is_none());
        assert_eq!(stages["ingestion"]["handlers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn mark_stage_running_updates_status() {
        let mut stages = json!({
            "ingestion": { "status": "pending" }
        });
        mark_stage_running(&mut stages, "ingestion");
        assert_eq!(stages["ingestion"]["status"], "running");
        // No started_at — timestamps are non-deterministic and break Restate replay
        assert!(stages["ingestion"].get("started_at").is_none());
    }

    #[test]
    fn mark_stage_complete_with_results() {
        let mut stages = json!({
            "ingestion": { "status": "running" }
        });
        let results = vec![
            HandlerResult {
                name: "Github".into(),
                status: StageStatus::Completed,
                items: Some(142),
                error: None,
            },
            HandlerResult {
                name: "Jira".into(),
                status: StageStatus::Failed,
                items: None,
                error: Some("auth expired".into()),
            },
        ];
        mark_stage_complete(&mut stages, "ingestion", &results);

        assert_eq!(stages["ingestion"]["status"], "completed");
        // No completed_at — timestamps are non-deterministic and break Restate replay
        assert!(stages["ingestion"].get("completed_at").is_none());
        let handlers = stages["ingestion"]["handlers"].as_array().unwrap();
        assert_eq!(handlers.len(), 2);
        assert_eq!(handlers[0]["items"], 142);
        assert_eq!(handlers[1]["error"], "auth expired");
    }

    #[test]
    fn mark_remaining_cancelled_only_affects_pending() {
        let mut stages = json!({
            "ingestion": { "status": "completed" },
            "metrics": { "status": "running" },
            "enrichment": { "status": "pending" },
            "embedding": { "status": "pending" }
        });
        mark_remaining_cancelled(&mut stages);

        assert_eq!(stages["ingestion"]["status"], "completed");
        assert_eq!(stages["metrics"]["status"], "running");
        assert_eq!(stages["enrichment"]["status"], "cancelled");
        assert_eq!(stages["embedding"]["status"], "cancelled");
    }
}
