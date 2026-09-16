use serde::{Deserialize, Serialize};
use std::fmt;

/// User roles in Merchant OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Employee,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "ADMIN",
            Role::Employee => "EMPLOYEE",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ADMIN" => Some(Role::Admin),
            "EMPLOYEE" => Some(Role::Employee),
            _ => None,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Strongly typed authenticated identity for Merchant OS.
///
/// Crucial Security Invariant:
/// - Private fields with controlled public accessors.
/// - Cannot be constructed by arbitrary external callers; only the `auth` module can issue it
///   upon successful authentication or initial admin setup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedIdentity {
    user_id: String,
    username: String,
    role: Role,
}

impl AuthenticatedIdentity {
    /// Internal constructor only available within the auth crate/module.
    pub(crate) fn new(user_id: String, username: String, role: Role) -> Self {
        Self {
            user_id,
            username,
            role,
        }
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}
