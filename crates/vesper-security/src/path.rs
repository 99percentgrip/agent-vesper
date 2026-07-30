use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

/// Stable identity for an explicitly authorized root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RootIdentity(String);

impl RootIdentity {
    /// Creates a root identity, not a filesystem path.
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            return Err("root identity is invalid");
        }
        Ok(Self(value))
    }

    /// Returns the opaque root identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated relative path with no traversal, root, or platform prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelativePath(String);

impl RelativePath {
    /// Validates a relative authority path.
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        let path = Path::new(&value);
        if value.is_empty()
            || value.len() > 4096
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err("relative path is invalid");
        }
        Ok(Self(value))
    }

    /// Returns the portable stored representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An authority descriptor tying a relative path to one explicit root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathCapability {
    /// Root identity.
    pub root: RootIdentity,
    /// Path relative to the authorized root.
    pub relative: RelativePath,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_capabilities_cannot_switch_roots_or_traverse() {
        let workspace = RootIdentity::new("workspace").unwrap();
        let cache = RootIdentity::new("cache").unwrap();
        assert_ne!(workspace, cache);
        assert!(RelativePath::new("../escape").is_err());
        assert!(RelativePath::new("/absolute").is_err());
        assert!(RelativePath::new("src/lib.rs").is_ok());
    }
}
