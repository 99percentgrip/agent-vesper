use std::collections::BTreeMap;

/// Environment after sensitive names have been removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrubbedEnvironment {
    values: BTreeMap<String, String>,
    removed_keys: Vec<String>,
}

impl ScrubbedEnvironment {
    /// Safe retained values.
    #[must_use]
    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.values
    }

    /// Removed key names. Values are never retained.
    #[must_use]
    pub fn removed_keys(&self) -> &[String] {
        &self.removed_keys
    }
}

/// Fail-closed sensitive environment-key classifier.
#[derive(Debug, Clone, Default)]
pub struct EnvironmentScrubber;

impl EnvironmentScrubber {
    /// Returns whether a key is sensitive.
    #[must_use]
    pub fn is_sensitive_key(key: &str) -> bool {
        let normalized = key.to_ascii_uppercase();
        normalized == "SSH_AUTH_SOCK"
            || [
                "API_KEY",
                "TOKEN",
                "SECRET",
                "PASSWORD",
                "PRIVATE",
                "ACCESS_KEY",
                "CREDENTIAL",
            ]
            .iter()
            .any(|marker| normalized.contains(marker))
    }

    /// Removes sensitive values while retaining only their key names for diagnostics.
    #[must_use]
    pub fn scrub<I, K, V>(entries: I) -> ScrubbedEnvironment
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut values = BTreeMap::new();
        let mut removed_keys = Vec::new();
        for (key, value) in entries {
            let key = key.into();
            if Self::is_sensitive_key(&key) {
                removed_keys.push(key);
            } else {
                values.insert(key, value.into());
            }
        }
        removed_keys.sort();
        ScrubbedEnvironment {
            values,
            removed_keys,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubber_removes_secret_families_and_ssh_agent() {
        let scrubbed = EnvironmentScrubber::scrub([
            ("PATH", "/bin"),
            ("ZAI_API_KEY", "canary"),
            ("SSH_AUTH_SOCK", "/tmp/socket"),
            ("customPassword", "canary"),
        ]);
        assert_eq!(
            scrubbed.values().get("PATH").map(String::as_str),
            Some("/bin")
        );
        assert_eq!(scrubbed.removed_keys().len(), 3);
        assert!(!format!("{scrubbed:?}").contains("canary"));
    }
}
