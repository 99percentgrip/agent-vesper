use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// A URL reduced to scheme, host, optional port, and a path placeholder.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RedactedUrl(String);

impl RedactedUrl {
    /// Parses and removes userinfo, query, fragment, and detailed path data.
    pub fn parse(value: &str) -> Result<Self, RedactedUrlError> {
        let parsed = Url::parse(value).map_err(|_| RedactedUrlError)?;
        let host = parsed.host_str().ok_or(RedactedUrlError)?;
        let port = parsed
            .port()
            .map_or_else(String::new, |value| format!(":{value}"));
        Ok(Self(format!("{}://{host}{port}/…", parsed.scheme())))
    }

    /// Returns the safe URL representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RedactedUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("RedactedUrl").field(&self.0).finish()
    }
}

impl fmt::Display for RedactedUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// URL was not an absolute host URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("endpoint URL is invalid")]
pub struct RedactedUrlError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitization_removes_all_common_secret_locations() {
        let url = RedactedUrl::parse(
            "https://user:password@example.test/private/token?api_key=canary#secret",
        )
        .unwrap();
        assert_eq!(url.as_str(), "https://example.test/…");
        let rendered = format!("{url:?}");
        for secret in ["user", "password", "private", "token", "canary", "secret"] {
            assert!(!rendered.contains(secret));
        }
    }
}
