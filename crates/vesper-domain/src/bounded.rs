use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

/// Error returned when a string violates its declared bound.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BoundedStringError {
    /// The value exceeded the maximum encoded byte length.
    #[error("value exceeds the maximum length of {maximum} bytes")]
    TooLong {
        /// Configured maximum.
        maximum: usize,
        /// Actual UTF-8 byte length.
        actual: usize,
    },
}

/// A UTF-8 string whose serialized representation has a compile-time byte bound.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoundedString<const MAX: usize>(String);

impl<const MAX: usize> BoundedString<MAX> {
    /// Validates and creates a bounded string.
    pub fn new(value: impl Into<String>) -> Result<Self, BoundedStringError> {
        let value = value.into();
        if value.len() > MAX {
            return Err(BoundedStringError::TooLong {
                maximum: MAX,
                actual: value.len(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the validated string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the declared maximum UTF-8 byte length.
    #[must_use]
    pub const fn maximum() -> usize {
        MAX
    }
}

impl<const MAX: usize> fmt::Debug for BoundedString<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<const MAX: usize> fmt::Display for BoundedString<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<const MAX: usize> Serialize for BoundedString<MAX> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for BoundedString<MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// User-visible text that must remain bounded in errors and diagnostics.
pub type SafeMessage = BoundedString<4096>;

/// One message content part. Larger persistence limits belong to session storage.
pub type ContentText = BoundedString<1_048_576>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialization_enforces_bounds() {
        let error = serde_json::from_str::<BoundedString<3>>(r#""four""#).unwrap_err();
        assert!(error.to_string().contains("maximum length"));
    }
}
