use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::{ExportOrgResponse, ImportOrgResponse};
use tonic::{Response, Status};

pub async fn handle_export_org(repos: &Repos) -> Result<Response<ExportOrgResponse>, Status> {
    let export = repos.org.export_org().await.map_err(|e| {
        tracing::error!(error = %e, "failed to export org");
        Status::internal("failed to export organisation")
    })?;

    let json_data = serde_json::to_vec_pretty(&export).map_err(|e| {
        tracing::error!(error = %e, "failed to serialize org export");
        Status::internal("failed to serialize organisation export")
    })?;

    Ok(Response::new(ExportOrgResponse { json_data }))
}

pub async fn handle_import_org(
    repos: &Repos,
    json_data: Vec<u8>,
    replace: bool,
) -> Result<Response<ImportOrgResponse>, Status> {
    let export: ps_core::repo::org::OrgExport =
        serde_json::from_slice(&json_data).map_err(|e| {
            tracing::warn!(error = %e, "invalid org export JSON");
            Status::invalid_argument(format!("invalid JSON: {e}"))
        })?;

    if export.version != 1 {
        return Err(Status::invalid_argument(format!(
            "unsupported export version: {} (expected 1)",
            export.version
        )));
    }

    let result = repos.org.import_org(&export, replace).await.map_err(|e| {
        tracing::error!(error = %e, "failed to import org");
        Status::internal("failed to import organisation")
    })?;

    Ok(Response::new(ImportOrgResponse {
        teams_created: result.teams_created,
        teams_updated: result.teams_updated,
        people_created: result.people_created,
        people_updated: result.people_updated,
        identities_created: result.identities_created,
        github_mappings_created: result.github_mappings_created,
        github_mappings_skipped: result.github_mappings_skipped,
        warnings: result.warnings,
    }))
}
