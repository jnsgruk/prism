mod config_export;
mod handler;

use std::collections::HashMap;

use ps_core::repo::Repos;
use tonic::Status;
use zeroize::Zeroizing;

use crate::common::{db_err, platform_to_proto, serde_json_to_prost_struct, to_timestamp};

pub struct ConfigServiceImpl {
    repos: Repos,
    secret_key: Zeroizing<[u8; 32]>,
    http_client: reqwest::Client,
}

impl ConfigServiceImpl {
    pub fn new(repos: Repos, secret_key: Zeroizing<[u8; 32]>) -> Self {
        Self {
            repos,
            secret_key,
            http_client: reqwest::Client::new(),
        }
    }
}

/// Build a `SourceConfig` proto from a DB row + secret status map.
pub(crate) fn build_source_proto(
    source: &ps_core::models::SourceConfig,
    secret_status: HashMap<String, bool>,
) -> ps_proto::canonical::prism::v1::SourceConfig {
    let settings_struct = serde_json_to_prost_struct(&source.settings);
    let (source_type, platform_instance) = platform_to_proto(&source.source_type.to_string());

    ps_proto::canonical::prism::v1::SourceConfig {
        id: source.id.to_string(),
        source_type,
        name: source.name.clone(),
        enabled: source.enabled,
        settings: Some(settings_struct),
        secret_status,
        schedule_cron: source.schedule_cron.clone(),
        created_at: Some(to_timestamp(source.created_at)),
        updated_at: Some(to_timestamp(source.updated_at)),
        platform_instance,
    }
}

/// Derive a URL-safe slug from a source name for use as a platform suffix.
///
/// "Canonical Discourse" → "canonical-discourse", "Ubuntu" → "ubuntu"
pub(crate) fn slugify_source_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) async fn fetch_secret_status(
    repos: &Repos,
    source_id: ps_core::models::SourceId,
) -> Result<HashMap<String, bool>, Status> {
    let keys = repos
        .config
        .list_secret_keys(source_id.into_inner())
        .await
        .map_err(db_err)?;
    Ok(keys.into_iter().map(|k| (k, true)).collect())
}
