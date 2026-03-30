use restate_sdk::prelude::TerminalError;
use uuid::Uuid;

use super::run_lifecycle::terminal_err;
use super::state::SharedState;

/// Decrypt a required secret. Returns an error if the secret is not configured.
///
/// Called outside `ctx.run()` to avoid journaling the plaintext.
pub async fn decrypt_required_secret(
    state: &SharedState,
    source_id: Uuid,
    key: &str,
) -> Result<String, TerminalError> {
    let encrypted = state
        .repos
        .config
        .get_encrypted_secret(source_id, key)
        .await
        .map_err(terminal_err("db error"))?
        .ok_or_else(|| TerminalError::new(format!("source has no {key} configured")))?;

    let decrypted = ps_core::crypto::decrypt(&state.secret_key, &encrypted)
        .map_err(terminal_err("decrypt error"))?;

    String::from_utf8(decrypted).map_err(|e| TerminalError::new(format!("invalid encoding: {e}")))
}

/// Decrypt an optional secret. Returns `Ok(None)` if the secret is not configured.
///
/// Called outside `ctx.run()` to avoid journaling the plaintext.
pub async fn decrypt_optional_secret(
    state: &SharedState,
    source_id: Uuid,
    key: &str,
) -> Result<Option<String>, TerminalError> {
    let encrypted = state
        .repos
        .config
        .get_encrypted_secret(source_id, key)
        .await
        .map_err(terminal_err("db error"))?;

    match encrypted {
        Some(enc) => {
            let decrypted = ps_core::crypto::decrypt(&state.secret_key, &enc)
                .map_err(terminal_err("decrypt error"))?;
            let s = String::from_utf8(decrypted).map_err(terminal_err("invalid encoding"))?;
            Ok(Some(s))
        }
        None => Ok(None),
    }
}
