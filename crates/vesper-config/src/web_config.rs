//! VRO-14 PR-5: `[web]` scope configuration from `.agent-vesper/config.toml`.
//!
//! Same deliberately minimal, dependency-free TOML reader discipline as
//! `sandbox_config` (bounded tables, tolerant of unknown keys, forward
//! compatible). The `[web]` block is the **opt-in switch** for the five
//! web tools: when it is absent, or `enabled = false`, the harness
//! registers nothing and the agent loop pays zero runtime cost — no tool
//! definitions are advertised, no sandbox route is constructed.
//!
//! Keys (all optional):
//! - `enabled` (bool, default `false`) — registers the web tools.
//! - `respect_robots` (bool, default `true`) — robots-respecting fetches.
//! - `output_budget_bytes` (int, default 96 KiB, hard cap 512 KiB) —
//!   per-tool output truncation bound applied after conversion.
//! - `allowlist` (array of strings, default empty = all hosts allowed,
//!   subject to the egress gate's address-class denials) — host allowlist
//!   handed to the egress policy.
//! - `engine.fetch.enabled` (default true), `engine.render.enabled` and
//!   `interact.enabled` (default false), `driver.image` (immutable ID/digest),
//!   `user_agent`, and fail-closed sandbox/private-address settings.
//!
//! Nested `[web.engine.fetch]`, `[web.engine.render]`, `[web.interact]`,
//! `[web.driver]`, and `[web.sandbox]` tables are equivalent to dotted keys.
//!
//! A second `[web]` table is an error; malformed known values are errors;
//! everything unknown is ignored.

use std::path::Path;

/// Default per-tool output budget (PRD §1.7).
pub const DEFAULT_OUTPUT_BUDGET_BYTES: u64 = 96 * 1024;
/// Hard ceiling for a configured budget (PRD §1.7).
pub const MAX_OUTPUT_BUDGET_BYTES: u64 = 512 * 1024;

/// Parse failure, honestly named.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WebConfigError {
    /// The file could not be read.
    #[error("cannot read {path}: {reason}")]
    Read {
        /// Path attempted.
        path: String,
        /// Underlying reason.
        reason: String,
    },
    /// `[web]` was defined twice.
    #[error("[web] table defined twice")]
    DuplicateTable,
    /// A known key carried an unparseable value.
    #[error("malformed value for `{key}` in [web]: {value}")]
    MalformedValue {
        /// Offending key.
        key: String,
        /// Raw value text.
        value: String,
    },
}

/// One parsed `[web]` scope configuration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebScopeConfig {
    /// Whether the web tools are registered at all. The only path to
    /// `true` is an explicit project setting, through the app or TOML.
    pub enabled: bool,
    /// Separate opt-in for browser interaction; unavailable drivers refuse.
    pub interact_enabled: bool,
    /// Plain fetch engine gate.
    pub fetch_enabled: bool,
    /// Single headless escalation gate.
    pub render_enabled: bool,
    /// Immutable OCI driver image reference; never pulled at host boot.
    pub driver_image: Option<String>,
    /// Credential-free HTTP identification string.
    pub user_agent: String,
    /// Robots-respecting fetches (default true).
    pub respect_robots: bool,
    /// Per-tool output truncation bound.
    pub output_budget_bytes: u64,
    /// Host allowlist (empty = all hosts, egress class rules still apply).
    pub allowlist: Vec<String>,
}

impl Default for WebScopeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interact_enabled: false,
            fetch_enabled: true,
            render_enabled: false,
            driver_image: None,
            user_agent: "agent-vesper".into(),
            respect_robots: true,
            output_budget_bytes: DEFAULT_OUTPUT_BUDGET_BYTES,
            allowlist: Vec::new(),
        }
    }
}

impl WebScopeConfig {
    /// Whether this config activates the web tools.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled
    }

    /// The output budget clamped to the hard ceiling.
    #[must_use]
    pub fn clamped_budget(&self) -> u64 {
        self.output_budget_bytes.min(MAX_OUTPUT_BUDGET_BYTES)
    }
}

/// Reads `<root>/.agent-vesper/config.toml` and extracts the `[web]`
/// table. A missing file or a file without `[web]` yields the disabled
/// default — configuring nothing is a valid, common, zero-cost state.
///
/// # Errors
///
/// - [`WebConfigError::Read`] when the file exists but cannot be read.
/// - [`WebConfigError::DuplicateTable`] on a second `[web]`.
/// - [`WebConfigError::MalformedValue`] on an unparseable known value.
pub fn read_web_scope(root: &Path) -> Result<WebScopeConfig, WebConfigError> {
    let settings = root.join(".agent-vesper/web-settings.json");
    match std::fs::read(&settings) {
        Ok(bytes) => {
            let config: WebScopeConfig =
                serde_json::from_slice(&bytes).map_err(|error| WebConfigError::Read {
                    path: settings.display().to_string(),
                    reason: error.to_string(),
                })?;
            if config
                .driver_image
                .as_deref()
                .is_some_and(|image| !is_digest_pinned_image(image))
            {
                return Err(WebConfigError::MalformedValue {
                    key: "driver.image".into(),
                    value: "image must be digest-pinned".into(),
                });
            }
            return Ok(config);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(WebConfigError::Read {
                path: settings.display().to_string(),
                reason: error.to_string(),
            });
        }
    }
    let path = root.join(".agent-vesper").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WebScopeConfig::default());
        }
        Err(error) => {
            return Err(WebConfigError::Read {
                path: path.display().to_string(),
                reason: error.to_string(),
            });
        }
    };
    parse_web_table(&text)
}

/// Parses the `[web]` table out of config text.
///
/// # Errors
///
/// Same as [`read_web_scope`] minus the read failure.
pub fn parse_web_table(text: &str) -> Result<WebScopeConfig, WebConfigError> {
    let mut config = WebScopeConfig::default();
    let mut seen = false;
    let mut in_table = false;
    let mut in_interact = false;
    let mut seen_interact = false;
    let mut subsection = String::new();
    let mut subsections = std::collections::BTreeSet::new();
    for raw_line in text.lines() {
        let line = strip_comment(raw_line).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            subsection = match line.as_str() {
                "[web.engine.fetch]" => "engine.fetch.",
                "[web.engine.render]" => "engine.render.",
                "[web.driver]" => "driver.",
                "[web.sandbox]" => "sandbox.",
                _ => "",
            }
            .into();
            if !subsection.is_empty() && !subsections.insert(subsection.clone()) {
                return Err(WebConfigError::DuplicateTable);
            }
            in_interact = line.trim() == "[web.interact]";
            if in_interact {
                if seen_interact {
                    return Err(WebConfigError::DuplicateTable);
                }
                seen_interact = true;
            }
            in_table = line.trim() == "[web]";
            if in_table {
                if seen {
                    return Err(WebConfigError::DuplicateTable);
                }
                seen = true;
            }
            continue;
        }
        if !in_table && !in_interact && subsection.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let full_key = format!("{subsection}{}", key.trim());
        let key = full_key.as_str();
        let key = if in_interact {
            if key == "enabled" {
                "interact.enabled"
            } else {
                continue;
            }
        } else {
            key
        };
        let value = value.trim();
        match key {
            "engine.fetch.enabled" | "engine.render.enabled" => {
                let flag = parse_bool(value).ok_or_else(|| WebConfigError::MalformedValue {
                    key: key.into(),
                    value: value.into(),
                })?;
                if key == "engine.fetch.enabled" {
                    config.fetch_enabled = flag;
                } else {
                    config.render_enabled = flag;
                }
            }
            "driver.image" | "user_agent" => {
                let text = value
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
                    .ok_or_else(|| WebConfigError::MalformedValue {
                        key: key.into(),
                        value: "invalid string".into(),
                    })?;
                if key == "driver.image" {
                    if !is_digest_pinned_image(text) {
                        return Err(WebConfigError::MalformedValue {
                            key: key.into(),
                            value: "requires image@sha256:<64 lowercase hex>".into(),
                        });
                    }
                    config.driver_image = Some(text.into());
                } else {
                    config.user_agent = text.into();
                }
            }
            "deny_private_addresses" | "sandbox.allow_network"
                if parse_bool(value) != Some(true) =>
            {
                return Err(WebConfigError::MalformedValue {
                    key: key.into(),
                    value: "web requires private-address denial and an explicit network grant"
                        .into(),
                });
            }
            "sandbox.requirement" if value != "\"network\"" => {
                return Err(WebConfigError::MalformedValue {
                    key: key.into(),
                    value: "web requires network isolation".into(),
                });
            }
            "enabled" => match parse_bool(value) {
                Some(flag) => config.enabled = flag,
                None => {
                    return Err(WebConfigError::MalformedValue {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                }
            },
            "interact.enabled" => match parse_bool(value) {
                Some(flag) => config.interact_enabled = flag,
                None => {
                    return Err(WebConfigError::MalformedValue {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                }
            },
            "respect_robots" => match parse_bool(value) {
                Some(flag) => config.respect_robots = flag,
                None => {
                    return Err(WebConfigError::MalformedValue {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                }
            },
            "output_budget_bytes" => match value.parse::<u64>() {
                Ok(budget) => config.output_budget_bytes = budget,
                Err(_) => {
                    return Err(WebConfigError::MalformedValue {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                }
            },
            "allowlist" => match parse_string_array(value) {
                Some(hosts) => config.allowlist = hosts,
                None => {
                    return Err(WebConfigError::MalformedValue {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                }
            },
            _ => {} // unknown key: ignore, forward compatible
        }
    }
    Ok(config)
}

/// Strict immutable OCI reference validation (no tags disguised as digests).
#[must_use]
pub fn is_digest_pinned_image(image: &str) -> bool {
    if let Some(digest) = image.strip_prefix("sha256:") {
        return digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    }
    let Some((name, digest)) = image.split_once("@sha256:") else {
        return false;
    };
    !name.is_empty()
        && !name.starts_with('-')
        && !name.chars().any(char::is_whitespace)
        && digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Strips `# comments` outside double-quoted strings.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..index],
            _ => {}
        }
    }
    line
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Parses `["a", "b"]` (allows empty `[]`).
fn parse_string_array(value: &str) -> Option<Vec<String>> {
    let trimmed = value.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        let unquoted = item.strip_prefix('"')?.strip_suffix('"')?;
        if unquoted.is_empty() {
            return None;
        }
        out.push(unquoted.to_string());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    //! The `[web]` parse matrix: defaults, opt-in, malformed values,
    //! duplicates, and cross-table isolation.

    use super::*;

    #[test]
    fn malformed_saved_settings_do_not_fall_back_to_enabled_toml() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".agent-vesper");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "[web]\nenabled = true\n").unwrap();
        for body in [
            "{",
            "{\"driver_image\":\"image:latest\"}",
            "{\"enabled\":\"yes\"}",
        ] {
            std::fs::write(dir.join("web-settings.json"), body).unwrap();
            assert!(read_web_scope(root.path()).is_err());
        }
    }

    #[test]
    fn complete_nested_web_config_and_immutable_images() {
        let digest = "a".repeat(64);
        let text = format!(
            "[web]\nenabled = true\nuser_agent = \"vesper-test\"\ndeny_private_addresses = true\n[web.engine.fetch]\nenabled = false\n[web.engine.render]\nenabled = true\n[web.driver]\nimage = \"sha256:{digest}\"\n[web.sandbox]\nrequirement = \"network\"\nallow_network = true\n"
        );
        let config = parse_web_table(&text).unwrap();
        assert!(!config.fetch_enabled && config.render_enabled);
        assert_eq!(config.user_agent, "vesper-test");
        assert_eq!(config.driver_image, Some(format!("sha256:{digest}")));
        assert!(is_digest_pinned_image(&format!(
            "example/image@sha256:{digest}"
        )));
        for image in [
            "example/image:latest",
            "example/image@sha256:short",
            "sha256:",
        ] {
            assert!(!is_digest_pinned_image(image));
        }
        for text in [
            "[web]\ndeny_private_addresses = false",
            "[web.sandbox]\nallow_network = false",
            "[web.sandbox]\nrequirement = \"filesystem\"",
        ] {
            assert!(parse_web_table(text).is_err());
        }
    }

    #[test]
    fn missing_table_yields_disabled_default() {
        let config = parse_web_table("[sandbox]\nnetwork = true\n").expect("parses");
        assert!(!config.enabled);
        assert!(config.respect_robots);
        assert_eq!(config.output_budget_bytes, DEFAULT_OUTPUT_BUDGET_BYTES);
        assert!(config.allowlist.is_empty());
        assert!(!config.is_active());
    }

    #[test]
    fn enabled_true_activates() {
        let config = parse_web_table("[web]\nenabled = true\n").expect("parses");
        assert!(config.enabled);
        assert!(config.is_active());
        assert!(!config.interact_enabled);
    }

    #[test]
    fn browser_opt_in_accepts_dotted_and_nested_forms() {
        for text in [
            "[web]\nenabled = true\ninteract.enabled = true\n",
            "[web]\nenabled = true\n[web.interact]\nenabled = true\n",
        ] {
            let config = parse_web_table(text).unwrap();
            assert!(config.enabled && config.interact_enabled);
        }
        assert!(parse_web_table("[web.interact]\nenabled = yes").is_err());
        assert!(parse_web_table("[web.interact]\n[web.interact]").is_err());
    }

    #[test]
    fn enabled_false_is_explicitly_off() {
        let config = parse_web_table("[web]\nenabled = false\n").expect("parses");
        assert!(!config.enabled);
        assert!(!config.is_active());
    }

    #[test]
    fn full_block_parses_every_key() {
        let text = concat!(
            "[web]\n",
            "enabled = true\n",
            "respect_robots = false\n",
            "output_budget_bytes = 131072\n",
            "allowlist = [\"example.com\", \"docs.example.org\"]\n",
            "# trailing comment\n",
        );
        let config = parse_web_table(text).expect("parses");
        assert!(config.enabled);
        assert!(!config.respect_robots);
        assert_eq!(config.output_budget_bytes, 131_072);
        assert_eq!(
            config.allowlist,
            vec!["example.com".to_string(), "docs.example.org".to_string()]
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let config = parse_web_table("[web]\nenabled = true\nfuture_key = 42\n")
            .expect("unknown key ignored");
        assert!(config.enabled);
    }

    #[test]
    fn duplicate_table_is_an_error() {
        let error = parse_web_table("[web]\n[web]\n").expect_err("duplicate");
        assert!(matches!(error, WebConfigError::DuplicateTable));
    }

    #[test]
    fn malformed_bool_is_an_error() {
        let error = parse_web_table("[web]\nenabled = yes\n").expect_err("bad bool");
        assert!(matches!(error, WebConfigError::MalformedValue { key, .. } if key == "enabled"));
    }

    #[test]
    fn malformed_budget_is_an_error() {
        let error = parse_web_table("[web]\noutput_budget_bytes = big\n").expect_err("bad budget");
        assert!(matches!(
            error,
            WebConfigError::MalformedValue { key, .. } if key == "output_budget_bytes"
        ));
    }

    #[test]
    fn malformed_allowlist_is_an_error() {
        let error = parse_web_table("[web]\nallowlist = [unquoted]\n").expect_err("bad allowlist");
        assert!(matches!(error, WebConfigError::MalformedValue { key, .. } if key == "allowlist"));
    }

    #[test]
    fn empty_allowlist_is_valid() {
        let config =
            parse_web_table("[web]\nenabled = true\nallowlist = []\n").expect("empty list ok");
        assert!(config.allowlist.is_empty());
    }

    #[test]
    fn comments_do_not_hide_values() {
        let config = parse_web_table("[web] # section comment\nenabled = true # inline\n")
            .expect("comments stripped");
        assert!(config.enabled);
    }

    #[test]
    fn other_tables_do_not_leak_keys() {
        let config = parse_web_table("[other]\nenabled = true\n").expect("parses");
        assert!(!config.enabled, "keys outside [web] must be ignored");
    }

    #[test]
    fn budget_clamps_to_ceiling() {
        let config = parse_web_table("[web]\noutput_budget_bytes = 99999999\n").expect("parses");
        assert_eq!(config.clamped_budget(), MAX_OUTPUT_BUDGET_BYTES);
    }

    #[test]
    fn missing_file_yields_disabled_default() {
        let root = std::env::temp_dir().join("vesper-web-config-missing");
        let config = read_web_scope(&root).expect("missing file is fine");
        assert!(!config.enabled);
    }
}
