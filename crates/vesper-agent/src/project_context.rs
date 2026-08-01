//! Bounded project instruction discovery for composed agent loops.
//!
//! The Python oracle progressively loads instruction files from the detected
//! project root. This provider-neutral port keeps the same shape while
//! enforcing a fixed byte budget, refusing symlinks, and redacting obvious
//! secret assignments before the text reaches a provider request.

use std::path::{Path, PathBuf};

use vesper_domain::{ContentPart, ContentText, SystemInstruction, WorkspaceRoot};

/// Maximum combined instruction bytes included in one provider request.
pub const MAX_PROJECT_CONTEXT_BYTES: usize = 32 * 1024;
const INSTRUCTION_NAMES: &[&str] = &[
    ".hermes.md",
    "HERMES.md",
    "AGENTS.md",
    "CLAUDE.md",
    "GLM.md",
    ".cursorrules",
];
const ROOT_MARKERS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "pyproject.toml",
    "package.json",
    "go.mod",
    "pom.xml",
    "Makefile",
];

/// Discovers bounded, progressively ordered project instructions.
#[must_use]
pub fn project_instructions(roots: &[WorkspaceRoot]) -> Vec<SystemInstruction> {
    let mut files = Vec::new();
    for root in roots {
        let path = PathBuf::from(root.path.as_str());
        let absolute = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        let project = project_root(&absolute);
        for directory in ancestors_between(&absolute, &project) {
            for name in INSTRUCTION_NAMES {
                let candidate = directory.join(name);
                if is_regular_file(&candidate) && !files.contains(&candidate) {
                    files.push(candidate);
                }
            }
        }
    }

    let mut remaining = MAX_PROJECT_CONTEXT_BYTES;
    files
        .into_iter()
        .filter_map(|path| {
            if remaining == 0 {
                return None;
            }
            let raw = std::fs::read_to_string(&path).ok()?;
            let text = redact_and_bound(&raw, remaining);
            if text.is_empty() {
                return None;
            }
            remaining = remaining.saturating_sub(text.len());
            let label = path
                .strip_prefix(project_root(&path))
                .ok()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    path.file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("instructions")
                });
            let body = format!("### {label}\n{text}");
            Some(SystemInstruction {
                content: vec![ContentPart::Text(ContentText::new(body).ok()?)],
                cache_stable: true,
                extensions: vesper_domain::ExtensionMap::default(),
            })
        })
        .collect()
}

fn project_root(start: &Path) -> PathBuf {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    let mut current = if start.is_file() {
        start.parent().unwrap_or(&start).to_path_buf()
    } else {
        start
    };
    loop {
        if ROOT_MARKERS
            .iter()
            .any(|marker| current.join(marker).exists())
        {
            return current;
        }
        let Some(parent) = current.parent() else {
            return current;
        };
        if parent == current {
            return current;
        }
        current = parent.to_path_buf();
    }
}

fn ancestors_between(start: &Path, root: &Path) -> Vec<PathBuf> {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let directory = if start.is_file() {
        start.parent().unwrap_or(&start)
    } else {
        &start
    };
    let mut ordered = Vec::new();
    let mut current = directory;
    loop {
        ordered.push(current.to_path_buf());
        if current == root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent;
    }
    ordered.reverse();
    ordered
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

fn redact_and_bound(raw: &str, remaining: usize) -> String {
    let mut output = String::new();
    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        let secret_assignment = [
            "api_key=",
            "api-key=",
            "token=",
            "password=",
            "secret=",
            "private_key=",
        ]
        .iter()
        .any(|marker| lower.contains(marker));
        let safe = if secret_assignment {
            "[redacted secret assignment]"
        } else {
            line
        };
        if output.len().saturating_add(safe.len()).saturating_add(1) > remaining {
            break;
        }
        output.push_str(safe);
        output.push('\n');
    }
    output.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn loads_parent_instructions_and_redacts_assignments() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("AGENTS.md"),
            "Prefer evidence\nAPI_KEY=do-not-forward\n",
        )
        .unwrap();
        fs::write(root.path().join("Cargo.toml"), "[package]\n").unwrap();
        let roots = vec![WorkspaceRoot {
            name: vesper_domain::BoundedString::new("workspace").unwrap(),
            path: vesper_domain::BoundedString::new(root.path().to_string_lossy()).unwrap(),
            primary: true,
        }];
        let output = project_instructions(&roots);
        let ContentPart::Text(text) = &output[0].content[0] else {
            panic!("instruction must be text");
        };
        let text = text.as_str();
        assert!(text.contains("Prefer evidence"));
        assert!(!text.contains("do-not-forward"));
    }
}
