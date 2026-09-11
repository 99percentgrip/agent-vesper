//! Bounded source capture. All included bytes are retained for the verifier;
//! worker edits to the live tree cannot alter an already captured check input.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(crate) struct SourceSnapshot {
    files: BTreeMap<PathBuf, (Vec<u8>, bool)>,
    pub digest: String,
}

impl SourceSnapshot {
    pub fn capture(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|_| "workspace unavailable")?;
        let mut stack = vec![(root.clone(), 0usize)];
        let mut files = BTreeMap::new();
        let mut bytes_total = 0usize;
        let mut entries = 0usize;
        while let Some((dir, depth)) = stack.pop() {
            if depth > 64 {
                return Err("source depth exceeds 64".into());
            }
            for entry in std::fs::read_dir(&dir).map_err(|_| "source inventory failed")? {
                let entry = entry.map_err(|_| "source inventory entry failed")?;
                entries += 1;
                if entries > 32768 {
                    return Err("source inventory exceeds 32768 entries".into());
                }
                let path = entry.path();
                let rel = path
                    .strip_prefix(&root)
                    .map_err(|_| "source escaped root")?
                    .to_path_buf();
                let name = entry.file_name();
                let name = name.to_str().ok_or("non-UTF8 source name")?;
                // Include known runtime configuration, never credentials or session stores.
                if dir.file_name().and_then(|n| n.to_str()) == Some(".agent-vesper")
                    && !matches!(
                        name,
                        "config.toml"
                            | "web-settings.json"
                            | "swarm-settings.json"
                            | "acceptance-settings.json"
                    )
                {
                    continue;
                }
                // Fixed generated/private roots are disclosed to the reviewer.
                if matches!(
                    name,
                    ".git" | "target" | "node_modules" | ".venv" | "__pycache__" | ".agent"
                ) {
                    continue;
                }
                if name == ".env" || name.starts_with(".env.") {
                    return Err(
                        "private .env input: move secrets outside the verification source root"
                            .into(),
                    );
                }
                let metadata = path
                    .symlink_metadata()
                    .map_err(|_| "source metadata failed")?;
                if metadata.file_type().is_symlink() {
                    return Err(format!("source symlink refused: {}", rel.display()));
                }
                if metadata.is_dir() {
                    stack.push((path, depth + 1));
                    continue;
                }
                if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 {
                    return Err("source special file or file larger than 8 MiB".into());
                }
                if path
                    .canonicalize()
                    .map_err(|_| "source canonicalization failed")?
                    != path
                {
                    return Err("source alias changed during capture".into());
                }
                let mut bytes = Vec::new();
                std::fs::File::open(&path)
                    .map_err(|_| "source open failed")?
                    .take(8 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "source read failed")?;
                bytes_total = bytes_total.saturating_add(bytes.len());
                if bytes.len() > 8 * 1024 * 1024
                    || bytes_total > 128 * 1024 * 1024
                    || files.len() >= 16384
                {
                    return Err("source snapshot exceeds 128 MiB or 16384 files".into());
                }
                #[cfg(unix)]
                let executable = {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode() & 0o111 != 0
                };
                #[cfg(not(unix))]
                let executable = false;
                files.insert(rel, (bytes, executable));
            }
        }
        let mut hash = Sha256::new();
        for (path, (bytes, executable)) in &files {
            let path = path.to_string_lossy().replace('\\', "/");
            hash.update((path.len() as u64).to_le_bytes());
            hash.update(path.as_bytes());
            hash.update([u8::from(*executable)]);
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        Ok(Self {
            files,
            digest: hex(&hash.finalize()),
        })
    }

    pub fn materialize(&self) -> Result<tempfile::TempDir, String> {
        let dir = tempfile::tempdir().map_err(|_| "cannot create verifier snapshot")?;
        for (path, (bytes, executable)) in &self.files {
            let destination = dir.path().join(path);
            std::fs::create_dir_all(destination.parent().ok_or("invalid snapshot path")?)
                .map_err(|_| "cannot create snapshot directory")?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(|_| "cannot create snapshot file")?;
            file.write_all(bytes).map_err(|_| "cannot write snapshot")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(if *executable {
                    0o500
                } else {
                    0o400
                }))
                .map_err(|_| "cannot protect snapshot file")?;
            }
            #[cfg(not(unix))]
            let _ = executable;
        }
        Ok(dir)
    }

    pub fn review_inventory(&self) -> String {
        let mut text = String::from(
            "Excluded generated/private roots: .git, target, node_modules, .venv, __pycache__, .agent, and private .agent-vesper state. Known config.toml/web/swarm/acceptance settings are included. If requirements depend on excluded inputs, report a blocking finding.\n",
        );
        for (path, (bytes, _)) in &self.files {
            text.push_str(&format!("{} ({} bytes)\n", path.display(), bytes.len()));
            if text.len() > 48 * 1024 {
                let mut end = 48 * 1024;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
                text.push_str(
                    "\nInventory truncated; use read-only tools to inspect additional paths.\n",
                );
                break;
            }
        }
        text
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
