use std::{ffi::OsStr, fmt};

use thiserror::Error;

/// Matches the shared domain identifier compatibility bound.
pub const MAX_SESSION_ID_BYTES: usize = 256;

/// Validated session filename generated without trusting input as a path.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionFileName(String);

impl SessionFileName {
    /// Applies the frozen Python `[a-zA-Z0-9_-]` policy and appends `.json`.
    pub fn from_requested_id(requested: &str) -> Result<Self, SessionFileNameError> {
        if requested.is_empty() {
            return Err(SessionFileNameError::Empty);
        }
        if requested.len() > MAX_SESSION_ID_BYTES {
            return Err(SessionFileNameError::TooLong {
                maximum: MAX_SESSION_ID_BYTES,
            });
        }
        if requested.contains('\0') {
            return Err(SessionFileNameError::NulByte);
        }
        let mut safe = String::with_capacity(requested.len() + 5);
        for character in requested.chars() {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                safe.push(character);
            } else {
                safe.push('_');
            }
        }
        safe.push_str(".json");
        Ok(Self(safe))
    }

    /// Validates an existing direct-child filename from a session directory.
    pub fn from_stored_name(name: &OsStr) -> Result<Self, SessionFileNameError> {
        let name = name.to_str().ok_or(SessionFileNameError::NonUtf8)?;
        let stem = name
            .strip_suffix(".json")
            .ok_or(SessionFileNameError::NotSessionJson)?;
        if stem.is_empty()
            || stem.len() > MAX_SESSION_ID_BYTES
            || !stem.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
        {
            return Err(SessionFileNameError::UnsafeStoredName);
        }
        Ok(Self(name.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn session_id_text(&self) -> &str {
        self.0
            .strip_suffix(".json")
            .expect("validated session filename always ends in .json")
    }
}

impl fmt::Debug for SessionFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SessionFileName")
            .field(&self.0)
            .finish()
    }
}

/// Invalid external ID or stored filename.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SessionFileNameError {
    #[error("session ID must not be empty")]
    Empty,
    #[error("session ID exceeds {maximum} bytes")]
    TooLong { maximum: usize },
    #[error("session ID contains a NUL byte")]
    NulByte,
    #[error("stored session filename is not UTF-8")]
    NonUtf8,
    #[error("stored filename is not a session JSON file")]
    NotSessionJson,
    #[error("stored session filename violates the safe compatibility alphabet")]
    UnsafeStoredName,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_compatible_mapping_never_retains_path_syntax() {
        assert_eq!(
            SessionFileName::from_requested_id("../../etc/passwd")
                .unwrap()
                .as_str(),
            "______etc_passwd.json"
        );
        assert_eq!(
            SessionFileName::from_requested_id("/absolute")
                .unwrap()
                .as_str(),
            "_absolute.json"
        );
        assert_eq!(
            SessionFileName::from_requested_id("session:id")
                .unwrap()
                .as_str(),
            "session_id.json"
        );
    }

    #[test]
    fn nul_empty_and_unbounded_ids_are_rejected() {
        assert_eq!(
            SessionFileName::from_requested_id("bad\0id").unwrap_err(),
            SessionFileNameError::NulByte
        );
        assert!(SessionFileName::from_requested_id("").is_err());
        assert!(SessionFileName::from_requested_id(&"a".repeat(257)).is_err());
    }
}
