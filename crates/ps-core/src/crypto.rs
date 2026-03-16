use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use zeroize::Zeroizing;

use crate::Error;

const NONCE_LEN: usize = 12;

/// Encrypt plaintext using AES-256-GCM. Returns nonce prepended to ciphertext.
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| Error::Encryption(format!("invalid key: {e}")))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| Error::Encryption(format!("encryption failed: {e}")))?;

    let mut result = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt ciphertext that was encrypted with [`encrypt`]. Expects nonce
/// prepended to the ciphertext.
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, Error> {
    if data.len() < NONCE_LEN {
        return Err(Error::Encryption("ciphertext too short".into()));
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| Error::Encryption(format!("invalid key: {e}")))?;

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| Error::Encryption(format!("decryption failed: {e}")))
}

/// Load the 256-bit encryption key from the `PS_SECRET_KEY` environment variable
/// (base64-encoded). The returned key is wrapped in [`Zeroizing`] so it is
/// automatically zeroed when dropped.
pub fn load_secret_key() -> Result<Zeroizing<[u8; 32]>, Error> {
    let encoded = Zeroizing::new(
        std::env::var("PS_SECRET_KEY")
            .map_err(|_| Error::Encryption("PS_SECRET_KEY environment variable not set".into()))?,
    );

    let decoded = Zeroizing::new(
        STANDARD
            .decode(encoded.trim())
            .map_err(|e| Error::Encryption(format!("PS_SECRET_KEY is not valid base64: {e}")))?,
    );

    let arr: [u8; 32] = decoded.as_slice().try_into().map_err(|_| {
        Error::Encryption(format!(
            "PS_SECRET_KEY must be 32 bytes, got {}",
            decoded.len()
        ))
    })?;
    Ok(Zeroizing::new(arr))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        rand::fill(&mut key);
        key
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plaintext = b"github_token_abc123";

        let encrypted = encrypt(&key, plaintext).unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn different_nonces_per_encryption() {
        let key = test_key();
        let plaintext = b"same-plaintext";

        let e1 = encrypt(&key, plaintext).unwrap();
        let e2 = encrypt(&key, plaintext).unwrap();

        assert_ne!(e1, e2, "each encryption should use a unique nonce");
    }

    #[test]
    fn wrong_key_fails() {
        let key1 = test_key();
        let key2 = test_key();
        let plaintext = b"secret";

        let encrypted = encrypt(&key1, plaintext).unwrap();
        let result = decrypt(&key2, &encrypted);

        assert!(result.is_err());
    }

    #[test]
    fn truncated_ciphertext_fails() {
        let key = test_key();
        let result = decrypt(&key, &[0u8; 5]);
        assert!(result.is_err());
    }

    #[test]
    fn load_secret_key_from_env() {
        let key = test_key();
        let encoded = STANDARD.encode(key);

        unsafe { std::env::set_var("PS_SECRET_KEY", &encoded) };
        let loaded = load_secret_key().unwrap();
        unsafe { std::env::remove_var("PS_SECRET_KEY") };

        assert_eq!(*loaded, key);
    }
}
