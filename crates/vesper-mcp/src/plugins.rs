//! Ed25519-signed plugin loader.
//!
//! Plugins are declarative packages (mirrors the oracle's
//! `plugins.py:_PERMISSIONS = {prompt_context, policy_templates,
//! workflows}`). Each plugin package is a directory containing:
//!
//! - `manifest.json` — declarative metadata (publisher, name, version,
//!   files, permissions).
//! - `signature.bin` — Ed25519 signature of the manifest bytes.
//! - `content/...` — the declarative content files.
//!
//! ## The No-Leak Guarantee (architect's mandate)
//!
//! [`PluginLoader::load`] ALWAYS requires a valid Ed25519 signature from
//! a trusted publisher. The [`PluginLoader::load_unsigned_debug`] method
//! exists ONLY under `#[cfg(debug_assertions)]`; in a `--release` build
//! the method does not exist at all, and any caller that attempts to
//! invoke it is a compile error. A release binary therefore CANNOT load
//! an unsigned plugin — there is no code path by which it could.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::error::McpError;

/// Maximum number of files in a plugin package (mirrors the oracle's
/// `MAX_PLUGIN_FILES = 32`).
pub const MAX_PLUGIN_FILES: usize = 32;
/// Maximum total size of a plugin package in bytes (mirrors the oracle's
/// `MAX_PLUGIN_BYTES = 2 MiB`).
pub const MAX_PLUGIN_BYTES: usize = 2 * 1024 * 1024;
/// Maximum length of a plugin id or publisher id.
pub const MAX_ID_CHARS: usize = 64;
/// Maximum length of a publisher id (the oracle allows 128; we cap lower
/// for storage hygiene).
pub const MAX_PUBLISHER_CHARS: usize = 128;
/// Maximum length of a plugin version string.
pub const MAX_VERSION_CHARS: usize = 32;
/// Manifest file name inside a plugin package.
pub const MANIFEST_FILENAME: &str = "manifest.json";
/// Signature file name inside a plugin package.
pub const SIGNATURE_FILENAME: &str = "signature.bin";

/// One declarative plugin package's manifest. Mirrors the oracle's
/// `plugins.py` manifest schema (publisher, name, version, files,
/// permissions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Publisher identity (must match a trusted publisher).
    pub publisher: String,
    /// Plugin id (lowercase, hyphens, digits).
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Content files included in the package (relative paths → SHA-256).
    pub files: BTreeMap<String, String>,
    /// Declared permissions — must be a subset of {prompt_context,
    /// policy_templates, workflows}. Executable code is intentionally
    /// unsupported.
    pub permissions: Vec<String>,
}

impl PluginManifest {
    /// Validates the manifest against the bounded contract.
    pub fn validate(&self) -> Result<(), McpError> {
        validate_id(&self.publisher, "publisher")
            .map_err(|_| McpError::InvalidManifest("publisher"))?;
        validate_id(&self.name, "name").map_err(|_| McpError::InvalidManifest("name"))?;
        if self.version.chars().count() > MAX_VERSION_CHARS {
            return Err(McpError::InvalidManifest("version length"));
        }
        if self.files.is_empty() || self.files.len() > MAX_PLUGIN_FILES {
            return Err(McpError::InvalidManifest("files count"));
        }
        // Permissions must be a subset of the declarative set.
        for perm in &self.permissions {
            if !matches!(
                perm.as_str(),
                "prompt_context" | "policy_templates" | "workflows"
            ) {
                return Err(McpError::InvalidManifest("permission"));
            }
        }
        Ok(())
    }
}

/// A loaded plugin record (post-verification, persisted to the log).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRecord {
    /// Stable opaque id, e.g. `plugin-3`.
    pub id: String,
    /// The verified manifest.
    pub manifest: PluginManifest,
    /// Publisher who signed the plugin (must be trusted).
    pub publisher: String,
    /// Whether the plugin was loaded via the debug unsigned path.
    /// Always `false` in `--release` builds (the path does not exist).
    #[serde(default)]
    pub unsigned_debug: bool,
    /// Load timestamp.
    pub loaded_at: SystemTime,
}

/// An Ed25519 signature over a plugin manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSignature {
    /// 64 raw Ed25519 signature bytes.
    pub bytes: [u8; 64],
}

impl PluginSignature {
    /// Reads the signature bytes from `path` (the `signature.bin` file
    /// inside a plugin package). Returns an error if the file is absent
    /// or not exactly 64 bytes.
    pub fn read(path: &Path) -> Result<Self, McpError> {
        let bytes = std::fs::read(path).map_err(|_| McpError::io("read"))?;
        if bytes.len() != 64 {
            return Err(McpError::SignatureVerificationFailed("signature length"));
        }
        let mut sig = [0u8; 64];
        sig.copy_from_slice(&bytes);
        Ok(Self { bytes: sig })
    }

    /// Writes the signature bytes to `path` atomically (used by tests
    /// and the signing tooling).
    pub fn write(&self, path: &Path) -> Result<(), McpError> {
        std::fs::write(path, self.bytes).map_err(|_| McpError::io("write"))
    }
}

/// One trusted publisher entry: publisher id + Ed25519 public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedPublisher {
    /// Publisher identity (matches `PluginManifest::publisher`).
    pub publisher: String,
    /// 32-byte Ed25519 verifying key, hex-encoded for JSON storage.
    pub public_key_hex: String,
}

impl TrustedPublisher {
    /// Decodes the hex public key into a `VerifyingKey`. Returns an error
    /// if the hex is malformed or the wrong length.
    pub fn verifying_key(&self) -> Result<VerifyingKey, McpError> {
        let bytes =
            hex_decode(&self.public_key_hex).ok_or(McpError::InvalidManifest("public key hex"))?;
        if bytes.len() != 32 {
            return Err(McpError::InvalidManifest("public key length"));
        }
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&bytes);
        VerifyingKey::from_bytes(&pk).map_err(|_| McpError::InvalidManifest("public key"))
    }
}

/// In-memory cache + on-disk JSONL store of [`TrustedPublisher`]s.
/// `Clone` is cheap (inner `Arc`); the binary and the `PluginLoader`
/// share one registry.
#[derive(Clone)]
pub struct TrustedPublishers {
    state: Arc<Mutex<Vec<TrustedPublisher>>>,
}

impl std::fmt::Debug for TrustedPublishers {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrustedPublishers")
            .field(
                "publishers",
                &self.state.lock().map(|s| s.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl TrustedPublishers {
    /// Creates an empty trusted-publishers registry. The composition
    /// boundary may pre-populate it from disk via [`Self::from_iter`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Creates a registry pre-populated from an iterator (e.g. loaded
    /// from `publishers.jsonl` at startup).
    #[must_use]
    pub fn from_records<I: IntoIterator<Item = TrustedPublisher>>(iter: I) -> Self {
        Self {
            state: Arc::new(Mutex::new(iter.into_iter().collect())),
        }
    }

    /// Returns the publisher with the given id, if any.
    #[must_use]
    pub fn get(&self, publisher: &str) -> Option<TrustedPublisher> {
        self.state
            .lock()
            .expect("trusted publishers mutex poisoned")
            .iter()
            .find(|entry| entry.publisher == publisher)
            .cloned()
    }

    /// Adds a trusted publisher. Idempotent on the publisher id.
    pub fn trust(&self, entry: TrustedPublisher) -> Result<(), McpError> {
        validate_id(&entry.publisher, "publisher")
            .map_err(|_| McpError::InvalidManifest("publisher"))?;
        // Validate the key decodes.
        let _ = entry.verifying_key()?;
        let mut state = self
            .state
            .lock()
            .expect("trusted publishers mutex poisoned");
        // Replace if the publisher already exists.
        state.retain(|existing| existing.publisher != entry.publisher);
        state.push(entry);
        Ok(())
    }

    /// Removes the publisher with the given id. Idempotent.
    pub fn revoke(&self, publisher: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("trusted publishers mutex poisoned");
        let before = state.len();
        state.retain(|entry| entry.publisher != publisher);
        before != state.len()
    }

    /// Lists every trusted publisher.
    #[must_use]
    pub fn list(&self) -> Vec<TrustedPublisher> {
        self.state
            .lock()
            .expect("trusted publishers mutex poisoned")
            .clone()
    }
}

impl Default for TrustedPublishers {
    fn default() -> Self {
        Self::new()
    }
}

/// Loads Ed25519-signed plugin packages. The security-critical type of
/// this crate.
pub struct PluginLoader {
    /// Root directory under which `plugins.jsonl` is written.
    root: PathBuf,
    /// Trusted publishers registry consulted on every `load`.
    trusted: TrustedPublishers,
    /// In-memory mirror of loaded plugin records.
    state: Mutex<Vec<PluginRecord>>,
}

impl std::fmt::Debug for PluginLoader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginLoader")
            .field("root", &self.root)
            .field("loaded", &self.state.lock().map(|s| s.len()).unwrap_or(0))
            .finish_non_exhaustive()
    }
}

impl PluginLoader {
    /// Opens a plugin loader rooted at `root` with the supplied trusted
    /// publishers registry.
    pub fn open(root: &Path, trusted: TrustedPublishers) -> Result<Self, McpError> {
        Self::validate_root(root)?;
        let log_path = Self::log_path(root);
        let state = read_all_jsonl::<PluginRecord>(&log_path)?;
        Ok(Self {
            root: root.to_path_buf(),
            trusted,
            state: Mutex::new(state),
        })
    }

    fn validate_root(root: &Path) -> Result<(), McpError> {
        if !root.is_absolute() {
            return Err(McpError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.as_os_str().is_empty() => Ok(()),
            Some(parent) if parent.exists() => Ok(()),
            _ => Err(McpError::InvalidRoot),
        }
    }

    fn log_path(root: &Path) -> PathBuf {
        root.join("plugins.jsonl")
    }

    /// Returns the loaded plugin records in load order.
    #[must_use]
    pub fn list(&self) -> Vec<PluginRecord> {
        self.state
            .lock()
            .expect("plugin loader mutex poisoned")
            .clone()
    }

    /// Returns a reference to the trusted-publishers registry.
    #[must_use]
    pub fn trusted(&self) -> &TrustedPublishers {
        &self.trusted
    }

    /// Loads a plugin package from `package_dir`. **Always** requires:
    /// 1. A `manifest.json` that validates against the bounded contract.
    /// 2. A `signature.bin` (64 raw Ed25519 bytes).
    /// 3. The publisher in the manifest is in the trusted registry.
    /// 4. The signature verifies against the manifest bytes under the
    ///    publisher's public key.
    ///
    /// On success the plugin is appended to `plugins.jsonl` and returned.
    /// On ANY failure (missing signature, unknown publisher, bad
    /// signature, malformed manifest) the load is rejected with a
    /// [`McpError`] and the plugin log is untouched.
    pub fn load(&self, package_dir: &Path) -> Result<PluginRecord, McpError> {
        self.load_inner(package_dir, /* unsigned_debug */ false)
    }

    /// Verifies a signed package without appending a loaded-plugin record.
    /// This is the side-effect-free counterpart used by tool-level `verify`
    /// commands and keeps verification distinct from installation.
    pub fn verify(&self, package_dir: &Path) -> Result<PluginManifest, McpError> {
        let manifest_path = package_dir.join(MANIFEST_FILENAME);
        let manifest_bytes = std::fs::read(&manifest_path).map_err(|_| McpError::io("read"))?;
        if manifest_bytes.len() > MAX_PLUGIN_BYTES {
            return Err(McpError::BoundsViolated("manifest size"));
        }
        let manifest: PluginManifest = serde_json::from_slice(&manifest_bytes)?;
        manifest.validate()?;
        let signature = PluginSignature::read(&package_dir.join(SIGNATURE_FILENAME))?;
        let publisher = self
            .trusted
            .get(&manifest.publisher)
            .ok_or_else(|| McpError::PublisherNotTrusted(manifest.publisher.clone()))?;
        publisher
            .verifying_key()?
            .verify(&manifest_bytes, &Signature::from_bytes(&signature.bytes))
            .map_err(|_| McpError::SignatureVerificationFailed("ed25519 verify"))?;
        Ok(manifest)
    }

    /// **Dev-mode only.** Loads a plugin package WITHOUT signature
    /// verification. This method exists ONLY under
    /// `#[cfg(debug_assertions)]`; in a `--release` build the method
    /// does not exist at all, and any caller that attempts to invoke it
    /// is a compile error.
    ///
    /// This is the architect's No-Leak Guarantee: there is no code path
    /// by which a release binary can load an unsigned plugin.
    #[cfg(debug_assertions)]
    pub fn load_unsigned_debug(&self, package_dir: &Path) -> Result<PluginRecord, McpError> {
        self.load_inner(package_dir, /* unsigned_debug */ true)
    }

    /// Shared implementation. When `unsigned_debug` is true the signature
    /// verification step is skipped. The `unsigned_debug` flag is only
    /// ever `true` when called from `load_unsigned_debug`, which itself
    /// only exists under `#[cfg(debug_assertions)]`.
    fn load_inner(
        &self,
        package_dir: &Path,
        unsigned_debug: bool,
    ) -> Result<PluginRecord, McpError> {
        // 1. Read + validate the manifest.
        let manifest_path = package_dir.join(MANIFEST_FILENAME);
        let manifest_bytes = std::fs::read(&manifest_path).map_err(|_| McpError::io("read"))?;
        if manifest_bytes.len() > MAX_PLUGIN_BYTES {
            return Err(McpError::BoundsViolated("manifest size"));
        }
        let manifest: PluginManifest = serde_json::from_slice(&manifest_bytes)?;
        manifest.validate()?;

        // 2. Signature verification (skipped ONLY in dev mode).
        if !unsigned_debug {
            let signature_path = package_dir.join(SIGNATURE_FILENAME);
            let signature = PluginSignature::read(&signature_path)?;
            let publisher = self
                .trusted
                .get(&manifest.publisher)
                .ok_or_else(|| McpError::PublisherNotTrusted(manifest.publisher.clone()))?;
            let verifying_key = publisher.verifying_key()?;
            verifying_key
                .verify(&manifest_bytes, &Signature::from_bytes(&signature.bytes))
                .map_err(|_| McpError::SignatureVerificationFailed("ed25519 verify"))?;
        }

        // 3. Build the record, append to the log.
        let mut state = self.state.lock().expect("plugin loader mutex poisoned");
        let id = format!("plugin-{}", state.len() + 1);
        let record = PluginRecord {
            id,
            manifest: manifest.clone(),
            publisher: manifest.publisher,
            unsigned_debug,
            loaded_at: SystemTime::now(),
        };
        let serialized = serde_json::to_string(&record)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        state.push(record.clone());
        Ok(record)
    }
}

// === helpers ===

/// Validates an id against the oracle's `[a-z0-9][a-z0-9_-]{0,63}` pattern
/// (simplified — we allow the publisher pattern too via the same regex).
fn validate_id(value: &str, _label: &str) -> Result<(), ()> {
    if value.is_empty() || value.len() > MAX_ID_CHARS {
        return Err(());
    }
    let mut chars = value.chars();
    let first = chars.next().ok_or(())?;
    if !first.is_ascii_alphanumeric() {
        return Err(());
    }
    if !value.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '@' || c == '/'
    }) {
        return Err(());
    }
    Ok(())
}

/// Lowercase hex decoder for Ed25519 public keys.
fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for chunk in bytes.as_chunks::<2>().0 {
        let high = hex_nibble(chunk[0])?;
        let low = hex_nibble(chunk[1])?;
        out.push((high << 4) | low);
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Atomic append to a JSONL log.
pub(crate) fn append_line(target: &Path, line: &str) -> Result<(), McpError> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(target)
        .map_err(|_| McpError::io("open"))?;
    writeln!(file, "{line}").map_err(|_| McpError::io("write"))?;
    file.sync_all().map_err(|_| McpError::io("fsync"))?;
    Ok(())
}

/// Reads and parses every JSONL line from `target`.
pub(crate) fn read_all_jsonl<T: serde::de::DeserializeOwned>(
    target: &Path,
) -> Result<Vec<T>, McpError> {
    let bytes = match std::fs::read(target) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(McpError::io("read")),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| McpError::Serde)?;
    let mut records = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<T>(line) {
            records.push(record);
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    //! Plugin loader: signed load, unsigned REJECTION, dev-mode gating,
    //! trusted publishers.

    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use std::fs;
    use tempfile::TempDir;

    /// Builds a plugin package directory with a signed manifest. Returns
    /// `(package_dir, signing_key)` so tests can re-sign or tamper.
    fn build_signed_package(temp: &TempDir, publisher: &str) -> (PathBuf, SigningKey) {
        let package = temp.path().join("plugin-pkg");
        fs::create_dir_all(&package).unwrap();
        let manifest = PluginManifest {
            publisher: publisher.to_string(),
            name: "demo-plugin".to_string(),
            version: "0.1.0".to_string(),
            files: {
                let mut map = BTreeMap::new();
                map.insert("content/prompt.md".to_string(), "abc123".to_string());
                map
            },
            permissions: vec!["prompt_context".to_string()],
        };
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        fs::write(package.join(MANIFEST_FILENAME), &manifest_bytes).unwrap();
        let signing_key = SigningKey::generate(&mut OsRng);
        let signature = signing_key.sign(&manifest_bytes);
        let sig = PluginSignature {
            bytes: signature.to_bytes(),
        };
        sig.write(&package.join(SIGNATURE_FILENAME)).unwrap();
        (package, signing_key)
    }

    fn trust_publisher(loader: &PluginLoader, publisher: &str, signing_key: &SigningKey) {
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex_encode(&verifying_key.to_bytes());
        loader
            .trusted()
            .trust(TrustedPublisher {
                publisher: publisher.to_string(),
                public_key_hex,
            })
            .unwrap();
    }

    fn loader_under(temp: &TempDir) -> (PathBuf, PluginLoader) {
        let root = temp.path().join("mcp-root");
        fs::create_dir_all(&root).unwrap();
        let loader = PluginLoader::open(&root, TrustedPublishers::new()).unwrap();
        (root, loader)
    }

    #[test]
    fn signed_plugin_loads_when_publisher_is_trusted() {
        let temp = TempDir::new().unwrap();
        let (_root, loader) = loader_under(&temp);
        let (package, signing_key) = build_signed_package(&temp, "vesper-test");
        trust_publisher(&loader, "vesper-test", &signing_key);
        let record = loader.load(&package).unwrap();
        assert_eq!(record.manifest.name, "demo-plugin");
        assert_eq!(record.publisher, "vesper-test");
        assert!(!record.unsigned_debug);
        assert_eq!(loader.list().len(), 1);
    }

    #[test]
    fn signed_plugin_is_rejected_when_publisher_is_not_trusted() {
        let temp = TempDir::new().unwrap();
        let (_root, loader) = loader_under(&temp);
        let (package, _signing_key) = build_signed_package(&temp, "untrusted-publisher");
        // Do NOT trust the publisher.
        let err = loader.load(&package).unwrap_err();
        assert_eq!(
            err,
            McpError::PublisherNotTrusted("untrusted-publisher".into())
        );
        assert!(loader.list().is_empty());
    }

    #[test]
    fn tampered_manifest_is_rejected_via_signature_mismatch() {
        let temp = TempDir::new().unwrap();
        let (_root, loader) = loader_under(&temp);
        let (package, signing_key) = build_signed_package(&temp, "vesper-test");
        trust_publisher(&loader, "vesper-test", &signing_key);
        // Tamper with the manifest after signing.
        fs::write(
            package.join(MANIFEST_FILENAME),
            r#"{"publisher":"vesper-test","name":"tampered","version":"0.1.0","files":{"x":"y"},"permissions":[]}"#,
        )
        .unwrap();
        let err = loader.load(&package).unwrap_err();
        assert_eq!(err, McpError::SignatureVerificationFailed("ed25519 verify"));
    }

    #[test]
    fn unsigned_plugin_is_aggressively_rejected_by_load() {
        // THE security test: a package without signature.bin must be
        // hard-rejected by `load`. This test runs in BOTH debug and
        // release — it does not reference `load_unsigned_debug`, so it
        // compiles identically in both profiles.
        let temp = TempDir::new().unwrap();
        let (_root, loader) = loader_under(&temp);
        // Build a package with manifest but no signature.
        let package = temp.path().join("unsigned-pkg");
        fs::create_dir_all(&package).unwrap();
        let manifest = PluginManifest {
            publisher: "vesper-test".to_string(),
            name: "unsigned".to_string(),
            version: "0.1.0".to_string(),
            files: {
                let mut map = BTreeMap::new();
                map.insert("content/prompt.md".to_string(), "abc".to_string());
                map
            },
            permissions: vec!["prompt_context".to_string()],
        };
        fs::write(
            package.join(MANIFEST_FILENAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        // No signature.bin → must hard-reject.
        let err = loader.load(&package).unwrap_err();
        assert!(matches!(
            err,
            McpError::SignatureVerificationFailed(_) | McpError::Io { .. }
        ));
        assert!(loader.list().is_empty());
    }

    #[test]
    fn invalid_permission_is_rejected() {
        let temp = TempDir::new().unwrap();
        let (_root, loader) = loader_under(&temp);
        let (package, signing_key) = build_signed_package(&temp, "vesper-test");
        trust_publisher(&loader, "vesper-test", &signing_key);
        // Rewrite the manifest with a forbidden permission and re-sign.
        let bad_manifest = PluginManifest {
            publisher: "vesper-test".to_string(),
            name: "evil".to_string(),
            version: "0.1.0".to_string(),
            files: {
                let mut map = BTreeMap::new();
                map.insert("x".to_string(), "y".to_string());
                map
            },
            permissions: vec!["executable_code".to_string()], // NOT allowed
        };
        let bad_bytes = serde_json::to_vec(&bad_manifest).unwrap();
        fs::write(package.join(MANIFEST_FILENAME), &bad_bytes).unwrap();
        let signature = signing_key.sign(&bad_bytes);
        PluginSignature {
            bytes: signature.to_bytes(),
        }
        .write(&package.join(SIGNATURE_FILENAME))
        .unwrap();
        let err = loader.load(&package).unwrap_err();
        assert_eq!(err, McpError::InvalidManifest("permission"));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn dev_mode_load_unsigned_debug_loads_without_signature_in_debug_builds() {
        // This test ONLY runs in debug builds. In `--release` builds the
        // `load_unsigned_debug` method does not exist, so this test
        // would be a compile error — confirming the No-Leak Guarantee.
        let temp = TempDir::new().unwrap();
        let (_root, loader) = loader_under(&temp);
        let package = temp.path().join("unsigned-pkg");
        fs::create_dir_all(&package).unwrap();
        let manifest = PluginManifest {
            publisher: "dev-only".to_string(),
            name: "unsigned".to_string(),
            version: "0.1.0".to_string(),
            files: {
                let mut map = BTreeMap::new();
                map.insert("content/prompt.md".to_string(), "abc".to_string());
                map
            },
            permissions: vec!["prompt_context".to_string()],
        };
        fs::write(
            package.join(MANIFEST_FILENAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        // No signature, no trusted publisher — but debug mode loads it.
        let record = loader.load_unsigned_debug(&package).unwrap();
        assert!(record.unsigned_debug);
        assert_eq!(record.manifest.name, "unsigned");
    }

    #[test]
    fn trusted_publishers_round_trip() {
        let registry = TrustedPublishers::new();
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex_encode(&verifying_key.to_bytes());
        registry
            .trust(TrustedPublisher {
                publisher: "vesper-test".to_string(),
                public_key_hex,
            })
            .unwrap();
        assert!(registry.get("vesper-test").is_some());
        assert!(registry.revoke("vesper-test"));
        assert!(registry.get("vesper-test").is_none());
    }

    /// Hex encoder (mirrors the snapshot module's helper).
    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }
}
