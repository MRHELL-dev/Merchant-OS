use std::fmt;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AuthError {
    /// Generic failure to avoid username enumeration
    InvalidCredentials,
    UserInactive,
    AdminAlreadyExists,
    AdminAuthorizationRequired(String),
    PermissionDenied {
        user_id: String,
        feature: String,
    },
    PasswordMismatch,
    WeakPassword(String),
    IncorrectSecurityAnswer,
    CryptoError(String),
    DatabaseError(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::InvalidCredentials => write!(f, "Invalid credentials"),
            AuthError::UserInactive => write!(f, "User account is inactive"),
            AuthError::AdminAlreadyExists => write!(f, "Initial Admin account already exists"),
            AuthError::AdminAuthorizationRequired(msg) => {
                write!(f, "Admin authorization required: {}", msg)
            }
            AuthError::PermissionDenied { user_id, feature } => write!(
                f,
                "Permission denied: user {} lacks permission for feature '{}'",
                user_id, feature
            ),
            AuthError::PasswordMismatch => write!(f, "Password confirmation does not match"),
            AuthError::WeakPassword(msg) => write!(f, "Password too weak: {}", msg),
            AuthError::IncorrectSecurityAnswer => write!(f, "Incorrect security answer"),
            AuthError::CryptoError(msg) => write!(f, "Cryptographic error: {}", msg),
            AuthError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<rusqlite::Error> for AuthError {
    fn from(err: rusqlite::Error) -> Self {
        AuthError::DatabaseError(err.to_string())
    }
}
