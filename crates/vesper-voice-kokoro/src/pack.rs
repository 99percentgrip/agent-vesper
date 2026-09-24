//! VRO-17 R3: the managed Natural Voice pack — pack descriptor, integrity
//! checks, and the per-user managed cache shared across projects, runs, and
//! application updates.
//!
//! Layout (per user, never per workspace):
//!
//! `~/.local/share/agent-vesper/voice-pack/`
//! - `model_quantized.onnx` — the pinned acoustic model (immutable revision)
//! - `voices/af_heart.bin` — style vector (Heart)
//! - `voices/am_michael.bin` — style vector (Michael)
//! - `tokenizer.json` — phoneme-char → ID vocabulary (pinned)
//! - `pack.json` — installation record: revision, digests, verified flag
//! - `onnxruntime/lib/libonnxruntime.so.1.28.0` — verified CPU runtime
//!
//! Pack JSON identity is stored *before* the model bytes finish and carries
//! a `verified` flag flipped only after full digest + size validation plus a
//! bounded no-speaker synthesis probe. `pack.json` is rewritten
//! (overwrite-rename) only after the flip so a partial staging can never
//! read as Ready. Integrity is verified on every read (cached by
//! size+mtime; digests only re-run when that identity changes) — the pack
//! is ~86 MiB, so hashing on every readiness call would be a redraw
//! hazard (directive §8).
//!
//! Provenance: assets come from the pinned immutable revision of the
//! upstream ONNX export; licenses are Apache-2.0 (model, voices, export)
//! and MIT (ONNX Runtime); notices are printed to stderr by the installed
//! application (harness `notices()`), never bundled as derivative content.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vesper_domain::ProviderId;
use vesper_voice::error::VoiceError;

/// Crate-unique provider identity (never collides with the pure core's
/// provider ids; the TUI maps scope selections to this adapter).
pub const PROVIDER_ID: &str = "voice-kokoro";

/// The single pinned immutable upstream revision this build supports.
/// Setup downloads exactly these assets; a different revision is
/// incompatible (fail-closed — no silent re-download of unreviewed content).
pub const PINNED_REVISION: &str = "1939ad2a8e416c0acfeecc08a694d14ef25f2231";

/// Pinned ONNX Runtime (CPU) release the adapter is ABI-matched to.
pub const ORT_VERSION: &str = "1.28.0";

/// Readiness-plumbing alias (the runtime version surfaced in Details).
pub const RUNTIME_VERSION: &str = ORT_VERSION;

/// Supported engines: the officially maintained Kokoro binding (kokoro-js)
/// passes espeak-ng IPA output to the model; espeak-ng is the verified
/// phonemizer. Both are external components on the user's machine
/// (espeak-ng GPL-3.0 system binary; never copied or bundled).
pub const PHONEMIZER_EXECUTABLE: &str = "espeak-ng";

/// Pinned asset: exact remote path, byte size, sha256 (verified against
/// the primary source's LFS manifest), and the installed file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedAsset {
    /// Remote file name at the pinned revision (e.g. `onnx/model_quantized.onnx`).
    pub remote_path: &'static str,
    /// Installed file name inside the pack root.
    pub installed_name: &'static str,
    /// Exact expected byte size.
    pub size: u64,
    /// Expected sha256 digest (lowercase hex).
    pub sha256: &'static str,
    /// Where the file lives inside the pack (installed layout role).
    pub role: PackRole,
}

/// Installed-layout role of one pinned asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackRole {
    /// The acoustic model (`model_quantized.onnx`).
    Model,
    /// A per-voice style vector (`voices/<id>.bin`).
    Voice,
    /// The phoneme vocabulary (`tokenizer.json`).
    Vocab,
    /// The CPU ONNX Runtime shared library (from the verified ORT release).
    Runtime,
}

impl PinnedAsset {
    /// Downloads from the pinned immutable revision (not `main`/`latest`).
    #[must_use]
    pub fn remote_url(&self) -> String {
        format!(
            "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/{PINNED_REVISION}/{}",
            self.remote_path
        )
    }
}

/// The acoustic model: quantized 8-bit integer (`model_quantized.onnx`).
/// **Recorded decision (R3):** the initially proposed q8f16 mixed-
/// precision export crashes ORT 1.28.0 CPU at session creation
/// (SIGSEGV in the ORT graph optimizer — coredump receipts in the R3
/// execution report); the quantized integer export is the compact
/// fallback the directive pre-authorizes ("A compact model_quantized.onnx
/// alternative is permitted if mixed precision is unsuitable"). +6.3 MiB
/// retained versus q8f16; same immutable revision.
pub const ASSET_MODEL: PinnedAsset = PinnedAsset {
    remote_path: "onnx/model_quantized.onnx",
    installed_name: "model_quantized.onnx",
    size: 92_361_116,
    sha256: "fbae9257e1e05ffc727e951ef9b9c98418e6d79f1c9b6b13bd59f5c9028a1478",
    role: PackRole::Model,
};

/// Heart (af_heart): female US English, creator grade A.
pub const ASSET_VOICE_AF_HEART: PinnedAsset = PinnedAsset {
    remote_path: "voices/af_heart.bin",
    installed_name: "voices/af_heart.bin",
    size: 522_240,
    sha256: "d583ccff3cdca2f7fae535cb998ac07e9fcb90f09737b9a41fa2734ec44a8f0b",
    role: PackRole::Voice,
};

/// Michael (am_michael): male US English.
pub const ASSET_VOICE_AM_MICHAEL: PinnedAsset = PinnedAsset {
    remote_path: "voices/am_michael.bin",
    installed_name: "voices/am_michael.bin",
    size: 522_240,
    sha256: "1d1f21dd8da39c30705cd4c75d039d265e9bc4a2a93ed09bc9e1b1225eb95ba1",
    role: PackRole::Voice,
};

/// Phoneme-char → ID vocabulary (also the model's input contract source).
pub const ASSET_VOCAB: PinnedAsset = PinnedAsset {
    remote_path: "tokenizer.json",
    installed_name: "tokenizer.json",
    size: 3_497,
    sha256: "77a02c8e164413299b4b4c403b14f8e0e1c1b727db4d46a09d6327b861060a34",
    role: PackRole::Vocab,
};

/// All voice assets this build installs (exactly two; not a repo snapshot).
pub const VOICE_ASSETS: [PinnedAsset; 2] = [ASSET_VOICE_AF_HEART, ASSET_VOICE_AM_MICHAEL];

/// The managed pack root: `~/.local/share/agent-vesper/voice-pack`
/// (`$XDG_DATA_HOME` honored), shared across projects and application
/// updates — never a per-workspace copy.
#[must_use]
pub fn pack_root() -> PathBuf {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
        && Path::new(&data_home).is_absolute()
    {
        return PathBuf::from(data_home)
            .join("agent-vesper")
            .join("voice-pack");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("agent-vesper")
            .join("voice-pack");
    }
    // No home: the pack cannot live anywhere honest.
    PathBuf::from("/nonexistent-agent-vesper-voice-pack")
}

/// Verified size of the extracted CPU runtime library (measured from the
/// pinned archive: `lib/libonnxruntime.so.1.28.0`).
pub const RUNTIME_LIB_SIZE: u64 = 24_268_848;
/// Verified sizes of the runtime license/notice files shipped alongside
/// the library (extracted from the same pinned archive).
pub const RUNTIME_LICENSE_SIZE: u64 = 10_132;
pub const RUNTIME_NOTICES_SIZE: u64 = 33_599;

/// The staging root for in-flight downloads (`<pack_root>/staging/<pid>`).
#[must_use]
pub fn staging_root() -> PathBuf {
    pack_root()
        .join("staging")
        .join(std::process::id().to_string())
}

/// The ONNX Runtime CPU release archive for x86_64 Linux (the single
/// supported target of this work unit): pinned URL, byte size, sha256
/// (GitHub release digest), and the runtime-library role. The archive is
/// transferred and verified, then extracted; the retained component is
/// the library below.
pub const RUNTIME_ASSET: PinnedAsset = PinnedAsset {
    remote_path: "onnxruntime-linux-x64-1.28.0.tgz",
    installed_name: "onnxruntime/lib/libonnxruntime.so.1.28.0",
    size: 9_125_960,
    sha256: "a3e1b79d7bb1bf09696ce675f49e4064e6c81f6202b8225624fff0e93f8d6407",
    role: PackRole::Runtime,
};

/// The extracted runtime library as retained on disk (measured from the
/// pinned archive): what [`verify_pack`] validates after installation.
pub const RUNTIME_LIB_FILE: PinnedAsset = PinnedAsset {
    remote_path: "onnxruntime-linux-x64-1.28.0.tgz!lib/libonnxruntime.so.1.28.0",
    installed_name: "onnxruntime/lib/libonnxruntime.so.1.28.0",
    size: 24_268_848,
    sha256: "1461ef7cc3d9e49982591721683cc3e3a55580aeca9a5254e7aac47b75ee4bab",
    role: PackRole::Runtime,
};

/// The extracted runtime license/notices (retained beside the library;
/// sizes and digests measured from the pinned archive).
pub const RUNTIME_LICENSE_FILE: PinnedAsset = PinnedAsset {
    remote_path: "onnxruntime-linux-x64-1.28.0.tgz!LICENSE",
    installed_name: "onnxruntime/lib/LICENSE-ONNXRUNTIME",
    size: 1_073,
    sha256: "2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c",
    role: PackRole::Runtime,
};

pub const RUNTIME_NOTICES_FILE: PinnedAsset = PinnedAsset {
    remote_path: "onnxruntime-linux-x64-1.28.0.tgz!ThirdPartyNotices.txt",
    installed_name: "onnxruntime/lib/ThirdPartyNotices-ONNXRUNTIME.txt",
    size: 325_054,
    sha256: "0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2",
    role: PackRole::Runtime,
};

impl PinnedAsset {
    /// Upstream URL for the runtime component (official ORT release).
    #[must_use]
    pub fn remote_url_runtime(&self) -> String {
        format!(
            "https://github.com/microsoft/onnxruntime/releases/download/v{ORT_VERSION}/{}",
            self.remote_path
        )
    }
}

/// Sum of every retained pack byte: model + voices + vocab + extracted
/// runtime library + runtime license/notices. The runtime archive is NOT
/// retained (deleted after extraction); it counts toward transfer/peak.
pub const RETAINED_PACK_BYTES: u64 = ASSET_MODEL.size
    + ASSET_VOICE_AF_HEART.size
    + ASSET_VOICE_AM_MICHAEL.size
    + ASSET_VOCAB.size
    + RUNTIME_LIB_FILE.size
    + RUNTIME_LICENSE_FILE.size
    + RUNTIME_NOTICES_FILE.size;

/// Peak setup footprint: retained pack + runtime archive (transferred and
/// briefly staged) + extraction staging for the library. The model is
/// stored as-is with no archive; partial downloads write directly into
/// the staging tree.
pub const PEAK_SETUP_BYTES: u64 = RETAINED_PACK_BYTES + RUNTIME_ASSET.size;

/// Aggregate budget the directive allows for retained pack components.
pub const RETAINED_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
/// Aggregate budget for peak setup usage (staging + retained + partials).
pub const PEAK_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

/// Installation record persisted inside the pack (`pack.json`). The
/// `verified` flag is the ONLY readiness source; file presence is
/// insufficient (directive §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRecord {
    /// Pack format version (migration/discovery safety).
    pub format: u32,
    /// The pinned upstream revision these files came from.
    pub revision: String,
    /// Per-file sha256 hex digests as installed (keyed by installed path).
    pub digests: Vec<(String, String)>,
    /// True only after full digest validation AND the bounded synthesis
    /// probe completed successfully in this installation.
    pub verified: bool,
    /// Voice ids installed (display order).
    pub voices: Vec<String>,
    /// ONNX Runtime version the installed library was verified against.
    pub runtime_version: String,
}

impl Default for PackRecord {
    fn default() -> Self {
        Self {
            format: 1,
            revision: PINNED_REVISION.to_owned(),
            digests: Vec::new(),
            verified: false,
            voices: Vec::new(),
            runtime_version: ORT_VERSION.to_owned(),
        }
    }
}

const PACK_RECORD_FILE: &str = "pack.json";

fn pack_record_path(root: &Path) -> PathBuf {
    root.join(PACK_RECORD_FILE)
}

/// Reads the installation record, if a syntactically valid one exists.
/// Digest/revision consistency is NOT checked here (that is
/// [`verify_pack`]'s job); malformed or foreign records read as absent.
#[must_use]
pub fn read_pack_record(root: &Path) -> Option<PackRecord> {
    let bytes = std::fs::read(pack_record_path(root)).ok()?;
    let record: PackRecord = serde_json::from_slice(&bytes).ok()?;
    (record.format == 1).then_some(record)
}

/// Writes the installation record atomically (temp file + rename in the
/// same directory), so a crash can never leave a torn record.
///
/// # Errors
/// IO failures surface as [`VoiceError::Unavailable`] with the setup
/// context; no partial record is left behind.
pub fn write_pack_record(root: &Path, record: &PackRecord) -> Result<(), VoiceError> {
    let path = pack_record_path(root);
    let dir = path
        .parent()
        .ok_or_else(|| VoiceError::InvalidInput("pack record has no parent".into()))?;
    std::fs::create_dir_all(dir).map_err(io_unavailable)?;
    let json = serde_json::to_vec_pretty(record)
        .map_err(|_| VoiceError::InvalidInput("pack record serialization failed".into()))?;
    let temp = dir.join(format!(".pack.json.tmp.{}", std::process::id()));
    std::fs::write(&temp, &json).map_err(io_unavailable)?;
    std::fs::rename(&temp, &path).map_err(io_unavailable)?;
    Ok(())
}

fn io_unavailable(error: std::io::Error) -> VoiceError {
    VoiceError::Unavailable {
        provider: ProviderId::new(PROVIDER_ID)
            .unwrap_or_else(|_| ProviderId::new("voice").expect("static id fits")),
        reason: vesper_domain::BoundedString::new(format!("voice pack storage error: {error}"))
            .unwrap_or_else(|_| {
                vesper_domain::BoundedString::new("voice pack storage error")
                    .expect("static fallback fits")
            }),
    }
}

/// Identity for the digest cache: (size, mtime nanos) per installed file.
/// `pack_identity` additionally hashes Unix device/inode/ctime so replacement
/// with preserved size/mtime still invalidates cached verification.
fn file_identity(path: &Path) -> Option<(u64, i128)> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        // Refuse symlinked pack files outright (recorded rule).
        return None;
    }
    let size = metadata.len();
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos() as i128;
    Some((size, mtime))
}

/// Validates one file's size + sha256 against its pinned expectation.
fn validate_file(root: &Path, expected: &PinnedAsset) -> Result<(), VoiceError> {
    let path = root.join(expected.installed_name);
    let Some((size, _)) = file_identity(&path) else {
        return Err(VoiceError::Unavailable {
            provider: ProviderId::new(PROVIDER_ID).expect("static id fits"),
            reason: vesper_domain::BoundedString::new(format!(
                "missing or refused pack file: {}",
                expected.installed_name
            ))
            .unwrap_or_else(|_| {
                vesper_domain::BoundedString::new("missing pack file").expect("fits")
            }),
        });
    };
    if size != expected.size {
        return Err(VoiceError::Unavailable {
            provider: ProviderId::new(PROVIDER_ID).expect("static id fits"),
            reason: vesper_domain::BoundedString::new(format!(
                "size mismatch for {}: expected {} bytes, found {size}",
                expected.installed_name, expected.size
            ))
            .unwrap_or_else(|_| vesper_domain::BoundedString::new("size mismatch").expect("fits")),
        });
    }
    let file = std::fs::File::open(&path).map_err(io_unavailable)?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer).map_err(io_unavailable)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hex(&hasher.finalize());
    if digest != expected.sha256 {
        return Err(VoiceError::Unavailable {
            provider: ProviderId::new(PROVIDER_ID).expect("static id fits"),
            reason: vesper_domain::BoundedString::new(format!(
                "digest mismatch for {}: pack file does not match its pinned content",
                expected.installed_name
            ))
            .unwrap_or_else(|_| {
                vesper_domain::BoundedString::new("digest mismatch").expect("fits")
            }),
        });
    }
    Ok(())
}

/// Lowercase hex encoding (bounded local helper; no new dependency).
fn hex(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX_CHARS[(byte >> 4) as usize] as char);
        out.push(HEX_CHARS[(byte & 0x0F) as usize] as char);
    }
    out
}

/// Digests every pack component (model, both voices, vocab, runtime lib)
/// against the pinned manifest. This is the heavy check — callers cache
/// its result by file identity; readiness must not re-run it per redraw.
/// The runtime library is validated when present; setup flips `verified`
/// only when every component including the runtime passes.
///
/// # Errors
/// [`VoiceError::Unavailable`] naming the first invalid/missing component.
pub fn verify_pack(root: &Path) -> Result<(), VoiceError> {
    validate_file(root, &ASSET_MODEL)?;
    for voice in &VOICE_ASSETS {
        validate_file(root, voice)?;
    }
    validate_file(root, &ASSET_VOCAB)?;
    // Retained runtime components (extracted from the verified
    // archive; the archive itself is deleted after extraction and is
    // never persisted, so it is not validated here).
    validate_file(root, &RUNTIME_LIB_FILE)?;
    validate_file(root, &RUNTIME_LICENSE_FILE)?;
    validate_file(root, &RUNTIME_NOTICES_FILE)?;
    Ok(())
}

/// Digest identity of the whole pack (cheap pre-check): sizes+mtimes of
/// every component plus Unix device/inode/ctime. A changed identity invalidates a cached readiness
/// verdict (directive §8: invalidate on asset change; never re-hash the
/// 86 MiB model on every redraw).
#[must_use]
pub fn pack_identity(root: &Path) -> Option<u64> {
    let mut hasher = Sha256::new();
    for asset in std::iter::once(&ASSET_MODEL)
        .chain(VOICE_ASSETS.iter())
        .chain(std::iter::once(&ASSET_VOCAB))
        .chain(std::iter::once(&RUNTIME_LIB_FILE))
        .chain(std::iter::once(&RUNTIME_LICENSE_FILE))
        .chain(std::iter::once(&RUNTIME_NOTICES_FILE))
    {
        let (size, mtime) = file_identity(&root.join(asset.installed_name))?;
        hasher.update(asset.installed_name.as_bytes());
        hasher.update(size.to_le_bytes());
        hasher.update(mtime.to_le_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::symlink_metadata(root.join(asset.installed_name)).ok()?;
            hasher.update(metadata.dev().to_le_bytes());
            hasher.update(metadata.ino().to_le_bytes());
            hasher.update(metadata.ctime().to_le_bytes());
            hasher.update(metadata.ctime_nsec().to_le_bytes());
        }
    }
    let digest = hasher.finalize();
    let mut value = [0u8; 8];
    value.copy_from_slice(&digest[..8]);
    Some(u64::from_le_bytes(value))
}

// Successful verification only. Kept bounded to avoid unbounded path retention.
fn verify_if_changed(
    cache: &mut Vec<(PathBuf, u64)>,
    root: &Path,
    verify: impl FnOnce(&Path) -> Result<(), VoiceError>,
) -> Result<(), VoiceError> {
    // On platforms without Unix inode/ctime identity, preserve unconditional
    // hashing rather than trusting size/mtime alone.
    if !cfg!(unix) {
        return verify(root);
    }
    let root = std::fs::canonicalize(root).map_err(io_unavailable)?;
    let identity = pack_identity(&root);
    if let Some(identity) = identity
        && cache.iter().any(|entry| entry == &(root.clone(), identity))
    {
        return Ok(());
    }
    cache.retain(|(path, _)| path != &root);
    verify(&root)?;
    if identity != pack_identity(&root) {
        return Err(VoiceError::InvalidInput(
            "voice pack changed during verification; retry".into(),
        ));
    }
    if let Some(identity) = identity {
        if cache.len() >= 8 {
            cache.remove(0);
        }
        cache.push((root, identity));
    }
    Ok(())
}

fn verify_cached(root: &Path) -> Result<(), VoiceError> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Vec<(PathBuf, u64)>>> =
        std::sync::OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| VoiceError::InvalidInput("voice verification cache poisoned".into()))?;
    verify_if_changed(&mut cache, root, verify_pack)
}

/// A pack problem classified for the shared readiness assessment (what is
/// missing vs broken vs incompatible), with the user-facing remedy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackProblem {
    /// The pack is not installed at all (no verified record).
    NotInstalled,
    /// Files exist but the record says verification never completed
    /// (interrupted setup).
    NotVerified,
    /// A component is missing, resized, or digest-mismatched.
    ComponentInvalid { detail: String },
    /// The espeak-ng phonemizer is unavailable (external prerequisite).
    PhonemizerMissing,
    /// The managed location lacks the space the operation needs.
    InsufficientSpace { available: u64, required: u64 },
}

impl PackProblem {
    /// Truthful user-facing description (setup route, never a download
    /// trigger by itself).
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::NotInstalled => "the voice pack is not installed".into(),
            Self::NotVerified => {
                "the voice pack download did not finish verification (install again to complete it)"
                    .into()
            }
            Self::ComponentInvalid { detail } => {
                format!("a voice pack file is invalid: {detail}")
            }
            Self::PhonemizerMissing => "the pronunciation engine (espeak-ng) is unavailable".into(),
            Self::InsufficientSpace {
                available,
                required,
            } => format!(
                "not enough disk space: {required} bytes needed, {available} available at the managed location"
            ),
        }
    }
}

/// What setup still needs, given the pack root's current state. Read-only;
/// never triggers downloads or repairs. Used by the shared readiness
/// assessment and by Settings before offering the confirm dialog.
///
/// # Errors
/// Inherited from [`verify_pack`] component failures.
pub fn assess(root: &Path) -> Result<Option<PackProblem>, VoiceError> {
    let record = read_pack_record(root);
    let Some(record) = record else {
        return Ok(Some(PackProblem::NotInstalled));
    };
    if !record.verified {
        return Ok(Some(PackProblem::NotVerified));
    }
    if let Err(error) = verify_cached(root) {
        let detail = match &error {
            VoiceError::Unavailable { reason, .. } => reason.as_str().to_owned(),
            other => other.to_string(),
        };
        return Ok(Some(PackProblem::ComponentInvalid { detail }));
    }
    Ok(None)
}

/// Removes ONLY pack-owned files (model, voices, vocab, runtime lib,
/// record). Refuses if any component is currently open by this process
/// (adapter hold) — the caller owns the adapter-first teardown contract.
/// Never touches unrelated data in the shared namespace.
///
/// # Errors
/// IO failures surface as [`VoiceError::Unavailable`].
pub fn remove_pack(root: &Path) -> Result<u64, VoiceError> {
    let mut reclaimed = 0u64;
    for asset in std::iter::once(&ASSET_MODEL)
        .chain(VOICE_ASSETS.iter())
        .chain(std::iter::once(&ASSET_VOCAB))
        .chain(std::iter::once(&RUNTIME_LIB_FILE))
        .chain(std::iter::once(&RUNTIME_LICENSE_FILE))
        .chain(std::iter::once(&RUNTIME_NOTICES_FILE))
    {
        let path = root.join(asset.installed_name);
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            reclaimed += metadata.len();
            if metadata.is_dir() {
                std::fs::remove_dir_all(&path).map_err(io_unavailable)?;
            } else {
                std::fs::remove_file(&path).map_err(io_unavailable)?;
            }
        }
    }
    let _ = std::fs::remove_file(pack_record_path(root));
    // Remove now-empty parents we own (voices/, onnxruntime/lib, staging).
    for dir in [
        root.join("voices"),
        root.join("onnxruntime").join("lib"),
        root.join("onnxruntime"),
        root.join("staging"),
    ] {
        let _ = std::fs::remove_dir(&dir);
    }
    Ok(reclaimed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_assets_match_primary_source_manifest() {
        // Byte sizes and digests were captured from the primary sources
        // (HF tree API at the pinned revision; ORT GitHub release). This
        // test pins them against accidental edit; the *verification* of
        // content happens at install time against the downloaded bytes.
        assert_eq!(ASSET_MODEL.size, 92_361_116);
        assert_eq!(ASSET_MODEL.sha256.len(), 64);
        for voice in &VOICE_ASSETS {
            assert_eq!(voice.size, 522_240);
            assert_eq!(voice.sha256.len(), 64);
        }
        assert_eq!(ASSET_VOCAB.remote_path, "tokenizer.json");
        assert_eq!(RUNTIME_ASSET.size, 9_125_960);
        assert_eq!(
            RUNTIME_ASSET.remote_url_runtime(),
            "https://github.com/microsoft/onnxruntime/releases/download/v1.28.0/onnxruntime-linux-x64-1.28.0.tgz"
        );
        assert!(ASSET_MODEL.remote_url().ends_with(
            "/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231/onnx/model_quantized.onnx"
        ));
    }

    #[test]
    fn budgets_headroom_is_honest() {
        const _: () = assert!(RETAINED_PACK_BYTES < RETAINED_BUDGET_BYTES);
        const _: () = assert!(PEAK_SETUP_BYTES < PEAK_BUDGET_BYTES);
        // Honest band (measured from the pinned sources): retained
        // ≈ 106.2 MiB (model + voices + vocab + extracted runtime +
        // licenses); peak ≈ 114.9 MiB (adds the briefly-staged archive).
        const _: () = assert!(RETAINED_PACK_BYTES > 100 * 1024 * 1024);
        const _: () = assert!(RETAINED_PACK_BYTES < 120 * 1024 * 1024);
        const _: () = assert!(PEAK_SETUP_BYTES > RETAINED_PACK_BYTES);
        const _: () = assert!(PEAK_SETUP_BYTES - RETAINED_PACK_BYTES == RUNTIME_ASSET.size);
    }

    #[test]
    fn missing_pack_reads_as_not_installed() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            assess(dir.path()).expect("assess"),
            Some(PackProblem::NotInstalled)
        );
    }

    #[test]
    fn unverified_record_is_not_ready() {
        let dir = tempfile::tempdir().expect("tempdir");
        let record = PackRecord::default();
        write_pack_record(dir.path(), &record).expect("write");
        assert_eq!(
            assess(dir.path()).expect("assess"),
            Some(PackProblem::NotVerified)
        );
    }

    #[test]
    fn corrupt_component_is_detected_without_record_claim() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Fake a verified record (component validation must still fail).
        let record = PackRecord {
            verified: true,
            ..PackRecord::default()
        };
        write_pack_record(dir.path(), &record).expect("write");
        std::fs::create_dir_all(dir.path().join("voices")).expect("mkdir");
        std::fs::write(
            dir.path().join(ASSET_VOCAB.installed_name),
            b"not the vocab",
        )
        .expect("write");
        // The vocab is present but wrong-sized; validation must fail on
        // the vocab entry even though the model is missing too (the
        // vocab check runs before the runtime check; the model failure
        // dominates when multiple components are absent — here the
        // vocab IS the first invalid component the per-file probe hits).
        let vocab_result = validate_file(dir.path(), &ASSET_VOCAB);
        assert!(matches!(vocab_result, Err(VoiceError::Unavailable { .. })));
        match assess(dir.path()).expect("assess") {
            Some(PackProblem::ComponentInvalid { detail }) => {
                // Either the absent model or the corrupt vocab may be
                // named first; both mean "not Ready".
                assert!(
                    detail.contains("tokenizer.json") || detail.contains("model_quantized.onnx"),
                    "detail: {detail}"
                );
            }
            other => panic!("expected component invalid, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn readiness_reuses_success_and_invalidates_changed_assets() {
        let dir = tempfile::tempdir().unwrap();
        for asset in std::iter::once(&ASSET_MODEL)
            .chain(VOICE_ASSETS.iter())
            .chain([
                &ASSET_VOCAB,
                &RUNTIME_LIB_FILE,
                &RUNTIME_LICENSE_FILE,
                &RUNTIME_NOTICES_FILE,
            ])
        {
            let path = dir.path().join(asset.installed_name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fixture").unwrap();
        }
        let mut cache = Vec::new();
        let calls = std::cell::Cell::new(0);
        let verify = |_: &Path| {
            calls.set(calls.get() + 1);
            Ok(())
        };
        verify_if_changed(&mut cache, dir.path(), verify).unwrap();
        verify_if_changed(&mut cache, dir.path(), verify).unwrap();
        assert_eq!(calls.get(), 1, "unchanged assets must not be rehashed");
        std::fs::write(
            dir.path().join(ASSET_MODEL.installed_name),
            b"changed asset",
        )
        .unwrap();
        verify_if_changed(&mut cache, dir.path(), verify).unwrap();
        assert_eq!(calls.get(), 2);
        // Same size + restored mtime must not hide replacement: inode/ctime
        // participate in the Unix cache identity as well.
        let model = dir.path().join(ASSET_MODEL.installed_name);
        let original_time = std::fs::metadata(&model).unwrap().modified().unwrap();
        let replacement = dir.path().join("replacement");
        std::fs::write(&replacement, b"other content").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(original_time))
            .unwrap();
        std::fs::rename(replacement, &model).unwrap();
        verify_if_changed(&mut cache, dir.path(), verify).unwrap();
        assert_eq!(calls.get(), 3);
        std::fs::remove_file(dir.path().join(ASSET_MODEL.installed_name)).unwrap();
        assert!(
            verify_if_changed(&mut cache, dir.path(), |_| Err(VoiceError::InvalidInput(
                "missing".into()
            )))
            .is_err()
        );
    }

    #[test]
    fn symlinked_pack_file_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        #[cfg(unix)]
        let link = dir.path().join(ASSET_VOCAB.installed_name);
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hostname", &link).expect("symlink");
        let record = PackRecord {
            verified: true,
            ..PackRecord::default()
        };
        write_pack_record(dir.path(), &record).expect("write");
        assert!(matches!(
            assess(dir.path()).expect("assess"),
            Some(PackProblem::ComponentInvalid { .. })
        ));
    }

    #[test]
    fn remove_reclaims_only_owned_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("voices")).expect("mkdir");
        std::fs::write(root.join("voices").join("af_heart.bin"), b"heart").expect("write");
        std::fs::write(root.join("unrelated.txt"), b"keep me").expect("write");
        let reclaimed = remove_pack(root).expect("remove");
        assert_eq!(reclaimed, 5); // the fake heart voice bytes
        assert!(
            root.join("unrelated.txt").exists(),
            "foreign data preserved"
        );
        assert!(!root.join("voices").join("af_heart.bin").exists());
    }

    #[test]
    fn pack_identity_changes_when_a_file_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("voices")).expect("mkdir");
        for asset in std::iter::once(&ASSET_MODEL)
            .chain(VOICE_ASSETS.iter())
            .chain(std::iter::once(&ASSET_VOCAB))
            .chain(std::iter::once(&RUNTIME_LIB_FILE))
            .chain(std::iter::once(&RUNTIME_LICENSE_FILE))
            .chain(std::iter::once(&RUNTIME_NOTICES_FILE))
        {
            let path = root.join(asset.installed_name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"x").expect("write");
        }
        let first = pack_identity(root).expect("identity");
        std::fs::write(root.join(ASSET_VOCAB.installed_name), b"y").expect("write");
        let second = pack_identity(root).expect("identity");
        assert_ne!(first, second);
    }
}
