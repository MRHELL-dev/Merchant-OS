use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

use super::error::AuthError;

/// Hashes a plaintext password using Argon2id with a cryptographically secure random salt.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    if password.len() < 6 {
        return Err(AuthError::WeakPassword(
            "Password must be at least 6 characters long".to_string(),
        ));
    }

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AuthError::CryptoError(e.to_string()))
}

/// Verifies a password against an Argon2id hash in constant time.
/// Returns false if the password does not match or if the hash is malformed.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AuthError> {
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return Ok(false),
    };

    let argon2 = Argon2::default();
    Ok(argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok())
}

/// Normalizes a security answer deterministically (trim -> lowercase) and hashes it using Argon2id.
pub fn hash_security_answer(answer: &str) -> Result<String, AuthError> {
    let normalized = answer.trim().to_lowercase();
    if normalized.is_empty() {
        return Err(AuthError::WeakPassword(
            "Security answer cannot be empty".to_string(),
        ));
    }

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    argon2
        .hash_password(normalized.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AuthError::CryptoError(e.to_string()))
}

/// Normalizes a candidate security answer and verifies it against the stored Argon2id hash.
pub fn verify_security_answer(answer: &str, hash: &str) -> Result<bool, AuthError> {
    let normalized = answer.trim().to_lowercase();
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return Ok(false),
    };

    let argon2 = Argon2::default();
    Ok(argon2.verify_password(normalized.as_bytes(), &parsed_hash).is_ok())
}
