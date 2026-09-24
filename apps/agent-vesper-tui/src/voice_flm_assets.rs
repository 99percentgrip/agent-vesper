//! VRO-17 R16: passive FLM asset verification for the composition
//! boundary. Read-only existence + identity checks against the
//! installed FLM user cache; **never** a vendor command execution at
//! probe time and **never** a download trigger.
//!
//! The verified installed layout (recorded in the owning report):
//! `~/.config/flm/models/Whisper-V3-Turbo-NPU2/` containing the four
//! catalog files, principal `model.q4nx` = 650,175,128 bytes with
//! SHA-256 `8fb97604…a16291`. The runtime is `/opt/fastflowlm/bin/flm`
//! (reachable through `/usr/bin/flm`).
//!
//! Why digest-vs-size: `flm check` executes the vendor binary (not a
//! passive probe), and mtime alone is not identity. The principal
//! weight is hashed only when its size already matches the pinned
//! value, so a normal readiness read never hashes 650 MB.

use std::path::{Path, PathBuf};

use vesper_domain::BoundedString;

/// The pinned principal weight identity (recorded installation).
const MODEL_FILE: &str = "model.q4nx";
const MODEL_BYTES: u64 = 650_175_128;
const MODEL_SHA256: &str = "8fb97604bf5762ee26efa696cfc9eb70724110358c7b4ca628a7973bf8a16291";
/// The catalog's required file set (beyond the principal weight).
const METADATA_FILES: [&str; 3] = ["config.json", "tokenizer.json", "tokenizer_config.json"];

/// The FLM model root (`FLM_MODEL_PATH` honored, mirroring the runtime's
/// own resolution; default `~/.config/flm/models`).
#[must_use]
pub fn flm_model_root() -> PathBuf {
    // Vendor semantics verified against installed 1.0.5: the env var is
    // the PARENT of the pack root (flm appends `models`), so a set
    // value resolves `<FLM_MODEL_PATH>/models`. Passing the pack root
    // itself to the vendor breaks catalog resolution (empty-JSON parse
    // error) — this function returns the pack parent either way.
    if let Some(root) = std::env::var_os("FLM_MODEL_PATH") {
        return PathBuf::from(root).join("models");
    }
    let Some(home) = std::env::var_os("HOME") else {
        return PathBuf::from("/nonexistent-flm-models");
    };
    PathBuf::from(home)
        .join(".config")
        .join("flm")
        .join("models")
}

/// The installed pack directory.
#[must_use]
pub fn whisper_pack_dir() -> PathBuf {
    flm_model_root().join("Whisper-V3-Turbo-NPU2")
}

/// Resolves the flm runtime executable passively: `/opt/fastflowlm/bin/flm`
/// if present, else the first `flm` on `PATH`. Never executes it.
#[must_use]
pub fn flm_executable() -> Option<PathBuf> {
    let direct = PathBuf::from("/opt/fastflowlm/bin/flm");
    if direct.is_file() {
        return Some(direct);
    }
    crate::voice_readiness::resolve_executable("flm")
}

/// One passive observation of the pack's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackState {
    /// Every catalog file present; principal weight size-matched and
    /// digest-verified against the pinned identity.
    Installed,
    /// The runtime is missing entirely (no setup can be offered for
    /// accelerated recognition on this machine).
    RuntimeMissing,
    /// The runtime exists but the pack directory/files are absent.
    AssetsMissing,
    /// The principal weight exists but its size mismatches the pinned
    /// value (a different revision — re-download requires consent).
    SizeMismatch { actual: u64 },
    /// The size matched but the digest did not (corrupt or replaced).
    DigestMismatch,
}

/// Passively assesses the pack (bounded; hashes the principal weight
/// only when its size already matches).
#[must_use]
pub fn assess_pack() -> PackState {
    if flm_executable().is_none() {
        return PackState::RuntimeMissing;
    }
    let dir = whisper_pack_dir();
    for name in METADATA_FILES {
        if !dir.join(name).is_file() {
            return PackState::AssetsMissing;
        }
    }
    let weights = dir.join(MODEL_FILE);
    let Ok(meta) = std::fs::metadata(&weights) else {
        return PackState::AssetsMissing;
    };
    if meta.len() != MODEL_BYTES {
        return PackState::SizeMismatch { actual: meta.len() };
    }
    if hash_file(&weights).as_deref() != Some(MODEL_SHA256) {
        return PackState::DigestMismatch;
    }
    PackState::Installed
}

fn hash_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1 << 20];
    use std::io::Read;
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => hasher.update(&buffer[..count]),
            Err(_) => return None,
        }
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

/// Maps a pack state to the bounded detail used by the adapter's spawn
/// path (setup guidance, never an opportunistic download). Pure: the
/// mapping is shared by the adapter and the tests.
pub fn verify_state_detail(state: PackState) -> Result<(), BoundedString<256>> {
    match state {
        PackState::Installed => Ok(()),
        PackState::RuntimeMissing => Err(BoundedString::new(
            "the accelerated recognizer's runtime is not installed; see Settings → Voice",
        )
        .expect("static fits")),
        PackState::AssetsMissing => Err(BoundedString::new(
            "the accelerated recognizer's model files are missing; install them in Settings → Voice",
        )
        .expect("static fits")),
        PackState::SizeMismatch { .. } => Err(BoundedString::new(
            "the installed accelerated-recognition model is a different revision; reinstall it in Settings → Voice",
        )
        .expect("static fits")),
        PackState::DigestMismatch => Err(BoundedString::new(
            "the installed accelerated-recognition model failed its integrity check; reinstall it in Settings → Voice",
        )
        .expect("static fits")),
    }
}

/// Verifies the actually-observed pack (adapter spawn path).
pub fn verify_installed() -> Result<(), BoundedString<256>> {
    verify_state_detail(assess_pack())
}
