use serde::{Deserialize, Serialize};

use crate::BoundedString;

/// Permission request identity.
pub type PermissionRequestId = BoundedString<128>;

/// Session permission mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionPermissionMode {
    /// Ask before authority-requiring operations.
    Ask,
    /// Skip interactive approval only after policy allows.
    Bypass,
    /// Permit read-only operations.
    ReadOnly,
}

/// Session operating mode, distinct from permission approval mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionOperatingMode {
    /// Normal coding session.
    Code,
    /// Planning-only session with source-compatible MCP allowance.
    Plan,
}

/// Terminal frontend/channel permission response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionOutcome {
    /// One-time approval.
    AllowOnce,
    /// One-time rejection.
    RejectOnce,
    /// Channel failed; policy must fail closed.
    ChannelFailure,
}
