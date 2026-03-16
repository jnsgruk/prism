use crate::Error;

/// Hash a password using Argon2id via the `password-auth` crate.
pub fn hash_password(password: &str) -> Result<String, Error> {
    Ok(password_auth::generate_hash(password))
}

/// Verify a password against a stored Argon2id hash.
pub fn verify_password(password: &str, hash: &str) -> Result<(), Error> {
    password_auth::verify_password(password, hash)
        .map_err(|_| Error::Authentication("invalid credentials".into()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let password = "correct-horse-battery-staple";
        let hash = hash_password(password).unwrap();

        assert!(hash.starts_with("$argon2"));
        verify_password(password, &hash).unwrap();
    }

    #[test]
    fn wrong_password_fails() {
        let hash = hash_password("right-password").unwrap();
        let result = verify_password("wrong-password", &hash);

        assert!(result.is_err());
    }

    #[test]
    fn different_hashes_for_same_password() {
        let hash1 = hash_password("same-password").unwrap();
        let hash2 = hash_password("same-password").unwrap();

        assert_ne!(hash1, hash2, "salted hashes should differ");
    }
}
