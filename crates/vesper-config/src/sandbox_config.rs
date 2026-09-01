//! VRO-13 PR-4: `[sandbox]` scope demands from `.agent-vesper/config.toml`.
//!
//! This is a **deliberately minimal, dependency-free TOML reader** for the
//! single `[sandbox]` table. The full workspace has no `toml` crate
//! dependency (adding one would violate the zero-new-mandatory-dependencies
//! directive for a config surface this small), and the frozen Python oracle
//! keeps `.agent-vesper/config.toml` out of scope entirely, so there is no
//! compatibility surface to mirror. Unknown keys and tables are ignored,
//! not errors: forward compatibility beats strictness here because a newer
//! config written by a future version must not break an older session.
//!
//! Parsing rules:
//! - `[sandbox]` starts a table; keys are `key = value` pairs.
//! - Values this reader understands: `true` / `false`, integers, floats,
//!   and bare strings. Anything else is ignored.
//! - Comments (`#` to end of line) are stripped outside quoted strings.
//! - A second `[sandbox]` table is an error (TOML disallows redefinition).
//!
//! The reader lives in `vesper-config` because that crate owns
//! configuration contracts (see its `AGENTS.md`), and it must be reachable
//! from both hosts without either depending on `vesper-sandbox`.

use std::path::Path;

use vesper_security::IsolationRequirement;

/// Parse failure, honestly named.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SandboxConfigError {
    /// The file could not be read.
    #[error("cannot read {path}: {reason}")]
    Read {
        /// Path attempted.
        path: String,
        /// Underlying reason.
        reason: String,
    },
    /// `[sandbox]` was defined twice.
    #[error("[sandbox] table defined twice")]
    DuplicateTable,
    /// A value could not be parsed at all.
    #[error("malformed value for `{key}` in [sandbox]: {value}")]
    MalformedValue {
        /// Offending key.
        key: String,
        /// Raw value text.
        value: String,
    },
}

/// One parsed `[sandbox]` scope demand.
///
/// Mirrors `vesper-agent`'s `SandboxDemand` shape (requirement + grants +
/// resource limits) without linking the two crates; `vesper-agent` owns
/// the *routing*, `vesper-config` owns the *file*.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SandboxScopeConfig {
    /// Demanded isolation class for shell-class tools. `None` (the
    /// default, and the zero-cost path) means no sandboxing is demanded.
    pub filesystem: bool,
    /// Explicit network grant inside the sandbox (egress allowed).
    /// Demanding isolation without this flag provisions with no network.
    pub network: bool,
    /// Demanded CPU quota (Docker `--cpus`). `None` = backend default.
    pub cpu_limit: Option<u32>,
    /// Demanded memory ceiling in MiB (Docker `--memory`). `None` =
    /// backend default.
    pub memory_limit_mib: Option<u32>,
}

impl SandboxScopeConfig {
    /// Resolves the strongest isolation requirement this config demands.
    ///
    /// `filesystem = true` alone demands `Filesystem` isolation. With
    /// `network = true` the demand is `Full` (process + filesystem +
    /// network-isolated with an egress grant). No keys set → `None`
    /// (no demand, byte-identical legacy path).
    #[must_use]
    pub fn resolved_requirement(&self) -> IsolationRequirement {
        match (self.filesystem, self.network) {
            (false, false) => IsolationRequirement::None,
            (true, false) => IsolationRequirement::Filesystem,
            // network grant without a filesystem demand still isolates the
            // process tree and network namespace; treat as Network demand.
            (false, true) => IsolationRequirement::Network,
            (true, true) => IsolationRequirement::Full,
        }
    }

    /// Whether this config activates the sandbox route at all.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(self.resolved_requirement(), IsolationRequirement::None)
    }
}

/// Reads `<root>/.agent-vesper/config.toml` and extracts the `[sandbox]`
/// table. A missing file or a file without `[sandbox]` yields the inactive
/// default — configuring nothing is a valid, common state.
///
/// # Errors
///
/// - [`SandboxConfigError::Read`] when the file exists but cannot be read.
/// - [`SandboxConfigError::DuplicateTable`] on a second `[sandbox]`.
/// - [`SandboxConfigError::MalformedValue`] on an unparseable value.
pub fn read_sandbox_scope(root: &Path) -> Result<SandboxScopeConfig, SandboxConfigError> {
    let path = root.join(".agent-vesper").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // A missing file is the common case: no config means no demand.
        // Only genuine I/O failures are errors.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SandboxScopeConfig::default());
        }
        Err(error) => {
            return Err(SandboxConfigError::Read {
                path: path.display().to_string(),
                reason: error.to_string(),
            });
        }
    };
    parse_sandbox_table(&text)
}

/// Parses the `[sandbox]` table out of TOML text.
///
/// # Errors
/// See [`read_sandbox_scope`].
pub fn parse_sandbox_table(text: &str) -> Result<SandboxScopeConfig, SandboxConfigError> {
    let mut config = SandboxScopeConfig::default();
    let mut in_sandbox = false;
    let mut seen_sandbox = false;

    for raw_line in text.lines() {
        let line = strip_comment(raw_line).trim().to_string();
        if line.is_empty() {
            continue;
        }
        // Table header: [sandbox] (exact; nested tables like [sandbox.x]
        // are not part of this contract and are skipped).
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].trim();
            in_sandbox = name == "sandbox";
            if in_sandbox {
                if seen_sandbox {
                    return Err(SandboxConfigError::DuplicateTable);
                }
                seen_sandbox = true;
            }
            continue;
        }
        if !in_sandbox {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue; // not a key = value line; ignore
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "filesystem" | "network" => {
                let flag = parse_bool(value).ok_or_else(|| SandboxConfigError::MalformedValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                })?;
                if key == "filesystem" {
                    config.filesystem = flag;
                } else {
                    config.network = flag;
                }
            }
            "cpu-limit" | "cpu_limit" => {
                let parsed =
                    value
                        .parse::<u32>()
                        .map_err(|_| SandboxConfigError::MalformedValue {
                            key: key.to_owned(),
                            value: value.to_owned(),
                        })?;
                config.cpu_limit = Some(parsed);
            }
            "memory-limit-mib" | "memory_limit_mib" => {
                let parsed =
                    value
                        .parse::<u32>()
                        .map_err(|_| SandboxConfigError::MalformedValue {
                            key: key.to_owned(),
                            value: value.to_owned(),
                        })?;
                config.memory_limit_mib = Some(parsed);
            }
            _ => {} // unknown keys are forward-compatibly ignored
        }
    }
    Ok(config)
}

/// Strips a trailing comment, respecting double-quoted strings.
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..index],
            _ => {}
        }
        let _ = in_string;
    }
    line
}

/// Parses a TOML boolean (`true`/`false` only; no case folding).
fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_sandbox_table_yields_inactive_default() {
        let config = parse_sandbox_table("").unwrap();
        assert!(!config.is_active());
        assert_eq!(config.resolved_requirement(), IsolationRequirement::None);
    }

    #[test]
    fn filesystem_true_demands_filesystem_isolation() {
        let config = parse_sandbox_table("[sandbox]\nfilesystem = true\n").unwrap();
        assert!(config.is_active());
        assert_eq!(
            config.resolved_requirement(),
            IsolationRequirement::Filesystem
        );
    }

    #[test]
    fn filesystem_plus_network_demands_full() {
        let config = parse_sandbox_table("[sandbox]\nfilesystem = true\nnetwork = true\n").unwrap();
        assert_eq!(config.resolved_requirement(), IsolationRequirement::Full);
        assert!(config.network);
    }

    #[test]
    fn network_only_demands_network_isolation() {
        let config = parse_sandbox_table("[sandbox]\nnetwork = true\n").unwrap();
        assert_eq!(config.resolved_requirement(), IsolationRequirement::Network);
    }

    #[test]
    fn resource_limits_parse_in_both_spellings() {
        let config = parse_sandbox_table(
            "[sandbox]\nfilesystem = true\ncpu-limit = 4\nmemory_limit_mib = 2048\n",
        )
        .unwrap();
        assert_eq!(config.cpu_limit, Some(4));
        assert_eq!(config.memory_limit_mib, Some(2048));
    }

    #[test]
    fn other_tables_are_ignored() {
        let config = parse_sandbox_table(
            "[providers.zai]\nfilesystem = true\n\n[sandbox]\nfilesystem = false\n",
        )
        .unwrap();
        assert!(!config.is_active());
    }

    #[test]
    fn comments_and_strings_are_respected() {
        let config = parse_sandbox_table(
            "# full-line comment\n[sandbox] # trailing comment\nfilesystem = true # flag\n",
        )
        .unwrap();
        assert!(config.filesystem);
    }

    #[test]
    fn duplicate_sandbox_table_is_an_error() {
        let error = parse_sandbox_table("[sandbox]\n[sandbox]\n").unwrap_err();
        assert_eq!(error, SandboxConfigError::DuplicateTable);
    }

    #[test]
    fn malformed_bool_is_an_error() {
        let error = parse_sandbox_table("[sandbox]\nfilesystem = yes\n").unwrap_err();
        assert!(matches!(error, SandboxConfigError::MalformedValue { .. }));
    }

    #[test]
    fn unknown_keys_are_forward_compatibly_ignored() {
        let config =
            parse_sandbox_table("[sandbox]\nfuture-key = \"value\"\nfilesystem = true\n").unwrap();
        assert!(config.filesystem);
    }

    #[test]
    fn read_missing_file_yields_inactive_default_not_error() {
        // A project without .agent-vesper/config.toml is the common case;
        // it must resolve to "no demand", never an error.
        let temp = tempfile::tempdir().unwrap();
        let config = read_sandbox_scope(temp.path()).unwrap();
        assert!(!config.is_active());
    }
}
