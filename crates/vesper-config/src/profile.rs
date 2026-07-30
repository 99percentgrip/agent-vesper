use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

/// Validated profile name with no traversal or separators.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileName(String);

impl ProfileName {
    /// Validates an Agent Vesper profile name.
    pub fn new(value: impl Into<String>) -> Result<Self, ProfileNameError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value != "."
            && value != ".."
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
            && value
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric());
        if !valid {
            return Err(ProfileNameError);
        }
        Ok(Self(value))
    }

    /// Returns the validated name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ProfileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ProfileName").field(&self.0).finish()
    }
}

impl Serialize for ProfileName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProfileName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Profile-name validation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("profile name must be 1-64 ASCII letters, digits, '-' or '_' and cannot traverse")]
pub struct ProfileNameError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_separators() {
        for value in ["", ".", "..", "../other", "a/b", r"a\b", "-leading"] {
            assert!(ProfileName::new(value).is_err(), "{value}");
        }
        assert!(ProfileName::new("work_2").is_ok());
    }
}
