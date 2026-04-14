use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::{ExportSourcesResponse, ImportSourcesResponse};
use tonic::{Response, Status};

pub async fn handle_export_sources(
    repos: &Repos,
) -> Result<Response<ExportSourcesResponse>, Status> {
    let export = repos.config.export_sources_portable().await.map_err(|e| {
        tracing::error!(error = %e, "failed to export sources");
        Status::internal("failed to export sources")
    })?;

    let json_data = serde_json::to_vec_pretty(&export).map_err(|e| {
        tracing::error!(error = %e, "failed to serialize sources export");
        Status::internal("failed to serialize sources export")
    })?;

    Ok(Response::new(ExportSourcesResponse { json_data }))
}

pub async fn handle_import_sources(
    repos: &Repos,
    json_data: Vec<u8>,
    replace: bool,
) -> Result<Response<ImportSourcesResponse>, Status> {
    let export: ps_core::repo::config::SourcesExport =
        serde_json::from_slice(&json_data).map_err(|e| {
            tracing::warn!(error = %e, "invalid sources export JSON");
            Status::invalid_argument(format!("invalid JSON: {e}"))
        })?;

    if export.version != 1 {
        return Err(Status::invalid_argument(format!(
            "unsupported export version: {} (expected 1)",
            export.version
        )));
    }

    let result = repos
        .config
        .import_sources(&export, replace)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to import sources");
            Status::internal("failed to import sources")
        })?;

    Ok(Response::new(ImportSourcesResponse {
        sources_created: result.sources_created,
        sources_skipped: result.sources_skipped,
        warnings: result.warnings,
    }))
}
