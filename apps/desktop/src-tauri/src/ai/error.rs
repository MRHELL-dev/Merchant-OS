use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AIError {
    OfflineUnavailable(String),
    ProviderError(String),
    TimeoutError,
    MalformedIntent(String),
    AmbiguousIntent(Vec<String>),
    ValidationFailure(String),
    SecurityRejection(String),
    DatabaseError(String),
}

impl fmt::Display for AIError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AIError::OfflineUnavailable(msg) => write!(f, "Offline intelligence unavailable: {}", msg),
            AIError::ProviderError(msg) => write!(f, "AI provider error: {}", msg),
            AIError::TimeoutError => write!(f, "AI provider request timed out"),
            AIError::MalformedIntent(msg) => write!(f, "Malformed structured intent: {}", msg),
            AIError::AmbiguousIntent(options) => {
                write!(f, "Ambiguous request. Candidates: {}", options.join(", "))
            }
            AIError::ValidationFailure(msg) => write!(f, "Intent validation failed: {}", msg),
            AIError::SecurityRejection(msg) => write!(f, "Security boundary rejection: {}", msg),
            AIError::DatabaseError(msg) => write!(f, "Database read error: {}", msg),
        }
    }
}

impl std::error::Error for AIError {}

impl From<rusqlite::Error> for AIError {
    fn from(err: rusqlite::Error) -> Self {
        AIError::DatabaseError(err.to_string())
    }
}
