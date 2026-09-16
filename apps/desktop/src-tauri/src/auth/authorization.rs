use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::error::AuthError;
use super::identity::{AuthenticatedIdentity, Role};

/// Stable permission identifiers for Merchant OS V1 capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionKey {
    Dashboard,
    Sales,
    Purchases,
    Inventory,
    Prices,
    Customers,
    CustomerCredits,
    CustomerOrders,
    Suppliers,
    Returns,
    Correction,
    Employees,
    Permissions,
    Expenses,
    TransactionHistory,
    Reports,
    BusinessProfile,
    BackupRestore,
}

impl PermissionKey {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionKey::Dashboard => "DASHBOARD",
            PermissionKey::Sales => "SALES",
            PermissionKey::Purchases => "PURCHASES",
            PermissionKey::Inventory => "INVENTORY",
            PermissionKey::Prices => "PRICES",
            PermissionKey::Customers => "CUSTOMERS",
            PermissionKey::CustomerCredits => "CUSTOMER_CREDITS",
            PermissionKey::CustomerOrders => "CUSTOMER_ORDERS",
            PermissionKey::Suppliers => "SUPPLIERS",
            PermissionKey::Returns => "RETURNS",
            PermissionKey::Correction => "CORRECTION",
            PermissionKey::Employees => "EMPLOYEES",
            PermissionKey::Permissions => "PERMISSIONS",
            PermissionKey::Expenses => "EXPENSES",
            PermissionKey::TransactionHistory => "TRANSACTION_HISTORY",
            PermissionKey::Reports => "REPORTS",
            PermissionKey::BusinessProfile => "BUSINESS_PROFILE",
            PermissionKey::BackupRestore => "BACKUP_RESTORE",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "DASHBOARD" => Some(PermissionKey::Dashboard),
            "SALES" => Some(PermissionKey::Sales),
            "PURCHASES" => Some(PermissionKey::Purchases),
            "INVENTORY" => Some(PermissionKey::Inventory),
            "PRICES" => Some(PermissionKey::Prices),
            "CUSTOMERS" => Some(PermissionKey::Customers),
            "CUSTOMER_CREDITS" => Some(PermissionKey::CustomerCredits),
            "CUSTOMER_ORDERS" => Some(PermissionKey::CustomerOrders),
            "SUPPLIERS" => Some(PermissionKey::Suppliers),
            "RETURNS" => Some(PermissionKey::Returns),
            "CORRECTION" => Some(PermissionKey::Correction),
            "EMPLOYEES" => Some(PermissionKey::Employees),
            "PERMISSIONS" => Some(PermissionKey::Permissions),
            "EXPENSES" => Some(PermissionKey::Expenses),
            "TRANSACTION_HISTORY" => Some(PermissionKey::TransactionHistory),
            "REPORTS" => Some(PermissionKey::Reports),
            "BUSINESS_PROFILE" => Some(PermissionKey::BusinessProfile),
            "BACKUP_RESTORE" => Some(PermissionKey::BackupRestore),
            _ => None,
        }
    }
}

/// The single authoritative Authorization Service for Merchant OS.
///
/// Rules:
/// - Admin is always unrestricted.
/// - Employee is authorized ONLY when a permission row exists and is_enabled = 1.
/// - Missing or disabled permission = DENIED.
pub struct AuthorizationService;

impl AuthorizationService {
    /// Authorizes a feature for the given authenticated identity.
    pub fn authorize(
        conn: &Connection,
        identity: &AuthenticatedIdentity,
        feature: &str,
    ) -> Result<(), AuthError> {
        // Admin is unrestricted across all operations
        if identity.is_admin() {
            return Ok(());
        }

        // Employee requires explicit enabled permission in permissions table
        if identity.role() == Role::Employee {
            let is_enabled: Option<i64> = conn
                .query_row(
                    "SELECT is_enabled FROM permissions WHERE user_id = ?1 AND feature_key = ?2",
                    params![identity.user_id(), feature],
                    |row| row.get(0),
                )
                .ok();

            if let Some(1) = is_enabled {
                return Ok(());
            }

            return Err(AuthError::PermissionDenied {
                user_id: identity.user_id().to_string(),
                feature: feature.to_string(),
            });
        }

        Err(AuthError::PermissionDenied {
            user_id: identity.user_id().to_string(),
            feature: feature.to_string(),
        })
    }

    /// Specifically enforces that the operation requires Admin authority.
    pub fn require_admin(
        identity: &AuthenticatedIdentity,
        operation: &str,
    ) -> Result<(), AuthError> {
        if identity.is_admin() {
            return Ok(());
        }

        Err(AuthError::AdminAuthorizationRequired(format!(
            "Operation '{}' requires Admin authority",
            operation
        )))
    }
}
