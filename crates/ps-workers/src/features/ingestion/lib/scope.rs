//! Validation shared by the coordinator and chunk snapshot boundaries.

use ps_core::ingestion::{PipelineScope, SelectedSource, SourceRunContext};
use ps_core::models::SourceConfig;
use restate_sdk::prelude::TerminalError;

pub(super) fn reject_unavailable_scope(request: &SourceRunContext) -> Result<(), TerminalError> {
    if !matches!(request.scope, PipelineScope::All) {
        return Err(TerminalError::new(
            "person adapters and processing are not available",
        ));
    }
    Ok(())
}

pub(super) fn validate_selected_config(
    config: &SourceConfig,
    selected: &SelectedSource,
) -> Result<(), TerminalError> {
    if config.id != selected.source_id
        || config.source_type != selected.platform
        || config.name != selected.source_name
    {
        return Err(TerminalError::new("selected source identity changed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ps_core::ingestion::{IdentitySnapshot, ProcessingScope};
    use ps_core::models::{PersonId, Platform, SourceId};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;
    use crate::features::ingestion::lib::chunk::ChunkRequest;

    fn selected() -> SelectedSource {
        SelectedSource {
            source_id: SourceId::now_v7(),
            source_name: "selected GitHub".into(),
            platform: Platform::Github,
            identity: None,
        }
    }

    fn source_context(source: SelectedSource) -> SourceRunContext {
        SourceRunContext {
            pipeline_id: Uuid::now_v7(),
            scope: PipelineScope::All,
            source,
            since_date: Some("2026-01-01".into()),
            run_started_at: OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
            processing: ProcessingScope::All,
        }
    }

    #[test]
    fn legacy_chunk_payload_keeps_original_wire_shape() {
        let value = serde_json::json!({"source_type":"github","cursor":"cursor","run_id":Uuid::now_v7(),"max_batches":50,"items_offset":12});
        let request: ChunkRequest = serde_json::from_value(value.clone()).unwrap();
        assert!(request.request.is_none());
        assert_eq!(serde_json::to_value(request).unwrap(), value);
    }

    #[test]
    fn multiple_chunks_preserve_owned_source_and_identity_snapshot() {
        let person_id = PersonId::now_v7();
        let mut context = source_context(selected());
        context.scope = PipelineScope::Person { person_id };
        context.processing = ProcessingScope::Person { person_id };
        context.source.identity = Some(IdentitySnapshot {
            identity_id: Uuid::now_v7(),
            person_id,
            platform: Platform::Github,
            username: "original-account".into(),
            platform_user_id: Some("account-id".into()),
        });
        for (cursor, offset) in [("first", 0), ("second", 50)] {
            let chunk = ChunkRequest {
                source_type: Platform::Github,
                cursor: cursor.into(),
                run_id: Uuid::now_v7(),
                max_batches: 50,
                items_offset: offset,
                request: Some(context.clone()),
            };
            let replayed: ChunkRequest =
                serde_json::from_value(serde_json::to_value(chunk).unwrap()).unwrap();
            assert_eq!(replayed.request.as_ref(), Some(&context));
        }
        assert!(reject_unavailable_scope(&context).is_err());
    }

    #[test]
    fn exact_source_binding_rejects_other_configs_of_the_same_platform() {
        let selected = selected();
        let mut config = SourceConfig {
            id: selected.source_id,
            source_type: selected.platform.clone(),
            name: selected.source_name.clone(),
            enabled: true,
            settings: serde_json::json!({}),
            schedule_cron: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        validate_selected_config(&config, &selected).unwrap();
        config.id = SourceId::now_v7();
        assert!(validate_selected_config(&config, &selected).is_err());
        config.id = selected.source_id;
        config.source_type = Platform::Discourse("different-instance".into());
        assert!(validate_selected_config(&config, &selected).is_err());
    }

    #[test]
    fn all_scope_can_use_selected_source_without_identity() {
        let request = source_context(selected());
        request.validate().unwrap();
        reject_unavailable_scope(&request).unwrap();
    }
}
