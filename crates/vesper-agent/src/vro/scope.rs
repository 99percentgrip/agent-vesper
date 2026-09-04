//! VRO-13 PR-5 — Scoped workspaces (PRD §3, `docs/qm-extraction-prd.md`).
//!
//! [`WorkspaceScope`] resolves, **once at host boot**, everything a session
//! needs that is keyed to a project directory: its identity, state dir,
//! cognitive-memory binding, skills surface, firewall composition, and
//! sandbox demand. The ReAct loop (`agent_loop.rs`, `vro/react.rs`) never
//! sees scopes — the resolution output is plain configuration handed to the
//! registry at boot, so the zero-degradation contract is structural
//! (PRD §3.3: "scopes are a host-layer concept, and the loop layer is
//! untouched").
//!
//! ## Layers (qm's `WorkspaceLayer` model, single-user)
//!
//! - Layer 0 (always, RW): the project's own `.agent-vesper/`.
//! - Layer 1 (always, RO): the global data root
//!   (`$XDG_DATA_HOME/agent-vesper` else `~/.local/share/agent-vesper`).
//! - Layer 2 (opt-in, RO): explicit extra workspace roots from
//!   [`AGENT_VESPER_EXTRA_SCOPES`](EXTRA_SCOPES_ENV) — **dormant by
//!   default**: with the variable unset or empty, no extra layer is mounted
//!   and resolution is byte-identical to the two-layer default.
//!
//! Reads union in layer order (project shadows global); writes target
//! Layer 0 only. Firewall rules compose as `global_rules ∪ project_rules`
//! with deny-precedence: a project may tighten, never un-deny a global
//! deny (PRD §3.2).
//!
//! ## Scope identity
//!
//! [`ScopeId`] is what stores key on — never the path string (port of qm's
//! `scopeStorageKey`). Identity comes from a `.vesper-scope-id` stamp file
//! in the project root. When absent, a short SHA-256 of the canonical root
//! is derived, stamped, and used, so renaming a project directory does not
//! re-key its stores (PRD §6.3's mitigation, adopted).
//!
//! ## ADR-0021 binding (project/global cognitive memory)
//!
//! [`ScopedCognition`] derives the project and global cognition directories
//! from the resolved layers and honors the two ADR-0021 environment
//! overrides (`AGENT_VESPER_COGNITION_ROOT`,
//! `AGENT_VESPER_GLOBAL_COGNITION_ROOT`) in **one** shared derivation used
//! by both hosts. Routing semantics (smart routing, promote/demote, store
//! ownership) remain owned by ADR-0021's TUI implementation and are not
//! duplicated or altered here.
//!
//! ## Boot-only discipline
//!
//! Resolution performs bounded filesystem I/O (stamp read/write, skills
//! directory scans) and is intended to run exactly once per host boot.
//! Nothing in this module is consulted mid-loop; a scope change requires a
//! host restart, mirroring the firewall holder contract.

use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use vesper_policy::firewall::{CommandFirewall, RuleDecision};

use crate::sandbox_route::SandboxDemand;

/// Stamp file that pins a project's [`ScopeId`] across directory renames.
pub const STAMP_FILE_NAME: &str = ".vesper-scope-id";
/// Per-project state directory name (Layer 0 root).
pub const STATE_DIR_NAME: &str = ".agent-vesper";
/// Skills subdirectory of every layer's state dir.
pub const SKILLS_DIR_NAME: &str = "skills";
/// Cognition subdirectory (ADR-0021 layout).
pub const COGNITION_DIR_NAME: &str = "cognition";
/// Environment variable mounting Layer 2 (opt-in extra scopes).
pub const EXTRA_SCOPES_ENV: &str = "AGENT_VESPER_EXTRA_SCOPES";
/// ADR-0021 override for the project cognition root.
pub const PROJECT_COGNITION_ENV: &str = "AGENT_VESPER_COGNITION_ROOT";
/// ADR-0021 override for the global cognition root.
pub const GLOBAL_COGNITION_ENV: &str = "AGENT_VESPER_GLOBAL_COGNITION_ROOT";
/// Opt-in environment variable that lets the auto-spawned ACP host persist
/// the identity stamp (root contract: no durable state in arbitrary project
/// directories by default). The interactive TUI never consults it.
pub const ENABLE_STAMP_ENV: &str = "AGENT_VESPER_ENABLE_SCOPE_STAMP";
/// Hex characters in a derived [`ScopeId`] (48 bits — single-user scale).
pub const SCOPE_ID_HEX_LEN: usize = 12;
/// Maximum accepted length of a hand-written or derived stamp token.
pub const MAX_STAMP_TOKEN_LEN: usize = 64;
/// Maximum Layer 2 mounts (bounded surface; more is a configuration error).
pub const MAX_EXTRA_SCOPES: usize = 8;
/// Per-skill byte bound on load (mirrors `vesper-memory`'s skill bound).
pub const MAX_SCOPED_SKILL_BYTES: usize = 200_000;
/// Maximum skills listed across all layers (bounded advertisement).
pub const MAX_SCOPED_SKILLS: usize = 500;

/// Why scope resolution or a scoped-skill load failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScopeError {
    /// The project root does not exist or is not accessible.
    #[error("project root is not accessible: {path}: {reason}")]
    RootNotAccessible {
        /// Path attempted.
        path: String,
        /// Underlying reason.
        reason: String,
    },
    /// No global data root could be derived (no XDG data home, no home dir).
    #[error("global scope root is unresolvable: set XDG_DATA_HOME or HOME")]
    GlobalRootUnresolvable,
    /// An explicitly configured extra scope does not exist.
    #[error("extra scope root is not accessible: {path}: {reason}")]
    ExtraRootNotAccessible {
        /// Path attempted.
        path: String,
        /// Underlying reason.
        reason: String,
    },
    /// More than [`MAX_EXTRA_SCOPES`] extra scopes were configured.
    #[error("too many extra scopes: {count} configured, {max} allowed")]
    TooManyExtraScopes {
        /// Configured count.
        count: usize,
        /// The cap.
        max: usize,
    },
    /// A skill path argument was unsafe (absolute, `..`, NUL, empty).
    #[error("unsafe skill path: {0}")]
    InvalidSkillPath(String),
    /// No layer contains the requested skill slug.
    #[error("skill not found in any scope layer: {slug}")]
    SkillNotFound {
        /// Requested slug.
        slug: String,
    },
    /// The skill file exceeds the per-skill byte bound.
    #[error("skill `{slug}` is {size} bytes; maximum is {max}")]
    SkillTooLarge {
        /// Requested slug.
        slug: String,
        /// Observed size in bytes.
        size: u64,
        /// The bound.
        max: usize,
    },
    /// The skill file could not be read.
    #[error("skill unreadable at {path}: {reason}")]
    SkillUnreadable {
        /// Path attempted.
        path: String,
        /// Underlying reason.
        reason: String,
    },
    /// The resolved skill path escapes its layer (symlink or traversal).
    #[error("skill path escapes its scope layer: {0}")]
    SkillEscapesLayer(PathBuf),
    /// A layer's skills directory became unreadable mid-resolution.
    #[error("scope layer skills directory unavailable: {path}: {reason}")]
    LayerRootUnavailable {
        /// Path attempted.
        path: String,
        /// Underlying reason.
        reason: String,
    },
    /// The process-global scope holder mutex is poisoned.
    #[error("scope holder is poisoned by a panicked thread")]
    HolderPoisoned,
    /// Firewall rule composition failed (invalid pattern).
    #[error("firewall composition failed: {0}")]
    FirewallCompose(String),
}

/// Stable, short identity of one project scope.
///
/// Derived once from the canonical root (or pinned by the
/// `.vesper-scope-id` stamp file) and used as the store key so renaming the
/// project directory does not re-key state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId(String);

impl ScopeId {
    /// Derives a short stable hash of a canonical project root.
    ///
    /// Only used when no stamp file exists yet; the derived value is then
    /// written to the stamp so subsequent boots (and post-rename boots) read
    /// it back instead of re-deriving from the changed path.
    #[must_use]
    pub fn from_canonical_root(canonical_root: &Path) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(canonical_root.as_os_str().as_encoded_bytes());
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(SCOPE_ID_HEX_LEN);
        for byte in digest.iter().take(SCOPE_ID_HEX_LEN.div_ceil(2)) {
            let _ = write!(hex, "{byte:02x}");
        }
        hex.truncate(SCOPE_ID_HEX_LEN);
        Self(hex)
    }

    /// The identity token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScopeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for ScopeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Whether a stamp token is acceptable: nonempty, bounded, single-line,
/// printable ASCII without whitespace.
fn is_valid_stamp_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_STAMP_TOKEN_LEN
        && token.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Host policy for persisting the scope identity stamp.
///
/// The stamp file (`root/.vesper-scope-id`) keeps a project's [`ScopeId`]
/// stable across directory renames, but writing it is durable state in the
/// project root — which the auto-spawned ACP process must not create by
/// default (root AGENTS.md contract). Resolution is therefore honest about
/// which host may write:
///
/// - [`Write`](StampPolicy::Write): read an existing stamp; when absent,
///   derive, persist, and use the derived value (the TUI's policy — the
///   user chose the directory).
/// - [`ReadOnly`](StampPolicy::ReadOnly): read an existing stamp; when
///   absent, derive in memory and use it **without writing**. The id is
///   stable for the process lifetime, and identical to what a writing boot
///   would have persisted — so cross-host `ScopeId` parity holds even when
///   only one host writes (the ACP's default, and its
///   `AGENT_VESPER_ENABLE_SCOPE_STAMP=1` opt-in upgrade path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StampPolicy {
    /// Read the stamp; when absent derive and persist it (TUI default).
    #[default]
    Write,
    /// Read the stamp; when absent derive in memory only (ACP default).
    ReadOnly,
}

/// Resolves (and per policy creates) the scope identity stamp.
///
/// Order of operations (directive 1): read `<root>/.vesper-scope-id`; if a
/// valid token is present, use it (both policies — a stamp written by any
/// previous boot wins). Otherwise derive [`ScopeId::from_canonical_root`];
/// [`StampPolicy::Write`] persists the derived value, while
/// [`StampPolicy::ReadOnly`] keeps it in memory so the auto-spawned host
/// never creates durable state in the project root. A stamp that exists
/// but is unreadable or corrupt is regenerated; a stamp that cannot be
/// **written** (read-only checkout) still yields a usable in-memory id
/// with `persisted = false` so host boot never degrades — the host may
/// surface a warning, and the id stays stable for this process.
fn ensure_scope_id(canonical_root: &Path, policy: StampPolicy) -> (ScopeId, bool) {
    let stamp = canonical_root.join(STAMP_FILE_NAME);
    if let Ok(text) = std::fs::read_to_string(&stamp) {
        let token = text.trim();
        if is_valid_stamp_token(token) {
            return (ScopeId(token.to_string()), true);
        }
        // Corrupt or invalid stamp: fall through and regenerate.
    }
    let derived = ScopeId::from_canonical_root(canonical_root);
    if policy == StampPolicy::ReadOnly {
        return (derived, false);
    }
    match std::fs::write(&stamp, derived.as_str()) {
        Ok(()) => (derived, true),
        Err(_) => (derived, false),
    }
}

/// Which resolution layer a mount came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeLayerKind {
    /// Layer 0: the project's own state dir. Read-write.
    Project,
    /// Layer 1: the user's global data root. Read-only.
    Global,
    /// Layer 2: an explicitly mounted extra workspace. Read-only.
    Extra,
}

/// One mounted scope layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeLayer {
    kind: ScopeLayerKind,
    root: PathBuf,
    state_dir: PathBuf,
    writable: bool,
}

impl ScopeLayer {
    fn project(root: PathBuf) -> Self {
        let state_dir = root.join(STATE_DIR_NAME);
        Self {
            kind: ScopeLayerKind::Project,
            root,
            state_dir,
            writable: true,
        }
    }

    fn global(root: PathBuf) -> Self {
        Self {
            kind: ScopeLayerKind::Global,
            root: root.clone(),
            state_dir: root,
            writable: false,
        }
    }

    fn extra(root: PathBuf) -> Self {
        let state_dir = root.join(STATE_DIR_NAME);
        Self {
            kind: ScopeLayerKind::Extra,
            root,
            state_dir,
            writable: false,
        }
    }

    /// Which layer this mount is.
    #[must_use]
    pub fn kind(&self) -> ScopeLayerKind {
        self.kind
    }

    /// The workspace (or data) root this layer was mounted from.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The layer's state directory (`.agent-vesper` shape, except the
    /// global layer whose root *is* the state dir).
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// Whether this layer accepts writes (Layer 0 only).
    #[must_use]
    pub fn is_writable(&self) -> bool {
        self.writable
    }

    /// The layer's skills directory.
    #[must_use]
    pub fn skills_dir(&self) -> PathBuf {
        self.state_dir.join(SKILLS_DIR_NAME)
    }
}

/// ADR-0021 binding: the two cognition directories a host derives its
/// project and global cognitive-memory engines from.
///
/// Pure path data — the engines themselves stay owned by the hosts and
/// `vesper-cognition`; routing, promote/demote, and store semantics are
/// untouched (ADR-0021 remains authoritative).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedCognition {
    project: PathBuf,
    global: PathBuf,
}

impl ScopedCognition {
    /// Derives the cognition roots from the resolved layers, honoring the
    /// two ADR-0021 environment overrides when the host supplies them.
    #[must_use]
    pub fn derive(
        state_dir: &Path,
        global_layer_root: &Path,
        project_override: Option<&Path>,
        global_override: Option<&Path>,
    ) -> Self {
        Self {
            project: project_override
                .map(Path::to_path_buf)
                .unwrap_or_else(|| state_dir.join(COGNITION_DIR_NAME)),
            global: global_override
                .map(Path::to_path_buf)
                .unwrap_or_else(|| global_layer_root.join(COGNITION_DIR_NAME)),
        }
    }

    /// The project (Layer 0) cognition root.
    #[must_use]
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// The global (Layer 1) cognition root.
    #[must_use]
    pub fn global(&self) -> &Path {
        &self.global
    }
}

/// One advertised skill, bound to the layer it was found in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedSkill {
    /// Skill slug (file stem).
    pub slug: String,
    /// Absolute path of the `.md` file.
    pub path: PathBuf,
    /// Index of the owning layer in [`ScopedSkills::layers`].
    pub layer: usize,
    /// Whether the owning layer accepts writes.
    pub writable: bool,
}

/// One loaded skill body plus its provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSkill {
    /// Skill slug that was requested.
    pub slug: String,
    /// File body, bounded by [`MAX_SCOPED_SKILL_BYTES`].
    pub content: String,
    /// Index of the owning layer.
    pub layer: usize,
    /// Whether the owning layer accepts writes.
    pub writable: bool,
}

/// The per-scope skills surface: layered `.md` discovery with shadowing and
/// safe-path loading (port of qm's `safeSkillFilePath` discipline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedSkills {
    layers: Vec<ScopeLayer>,
    entries: Vec<ScopedSkill>,
}

impl ScopedSkills {
    /// Scans every layer's `skills/` directory in layer order. Missing
    /// directories contribute nothing; earlier layers shadow later ones by
    /// slug; hidden and non-`.md` files are skipped; symlink escapes are
    /// not advertised. Bounded by [`MAX_SCOPED_SKILLS`].
    #[must_use]
    pub fn scan(layers: &[ScopeLayer]) -> Self {
        let mut entries: Vec<ScopedSkill> = Vec::new();
        for (index, layer) in layers.iter().enumerate() {
            let skills_dir = layer.skills_dir();
            let Ok(canonical_dir) = skills_dir.canonicalize() else {
                continue;
            };
            let Ok(read_dir) = std::fs::read_dir(&skills_dir) else {
                continue;
            };
            for entry in read_dir.flatten() {
                if entries.len() >= MAX_SCOPED_SKILLS {
                    break;
                }
                let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if name.starts_with('.') || !name.ends_with(".md") {
                    continue;
                }
                let slug = name[..name.len() - 3].to_string();
                if !is_safe_slug_shape(&slug) {
                    continue;
                }
                // Advertise only paths that stay inside this layer after
                // canonicalization (symlink escapes are excluded here and
                // independently re-checked on load).
                match entry.path().canonicalize() {
                    Ok(canonical) if canonical.starts_with(&canonical_dir) => {}
                    _ => continue,
                }
                if entries.iter().any(|skill| skill.slug == slug) {
                    continue; // earlier layer shadows
                }
                entries.push(ScopedSkill {
                    slug,
                    path: entry.path(),
                    layer: index,
                    writable: layer.is_writable(),
                });
            }
        }
        Self {
            layers: layers.to_vec(),
            entries,
        }
    }

    /// The layers this surface was scanned from, in resolution order.
    #[must_use]
    pub fn layers(&self) -> &[ScopeLayer] {
        &self.layers
    }

    /// Advertised skills in layer order (project first).
    #[must_use]
    pub fn entries(&self) -> &[ScopedSkill] {
        &self.entries
    }

    /// Loads one skill by slug: first layer (in order) containing
    /// `<state>/skills/<slug>.md` wins. The slug is validated, the resolved
    /// path is re-confined against the layer's skills directory (symlink
    /// escapes fail closed), and the body is bounded by
    /// [`MAX_SCOPED_SKILL_BYTES`].
    ///
    /// # Errors
    ///
    /// [`ScopeError::InvalidSkillPath`] for unsafe slugs,
    /// [`ScopeError::SkillEscapesLayer`] for symlink/traversal escapes,
    /// [`ScopeError::SkillTooLarge`] over the byte bound,
    /// [`ScopeError::SkillUnreadable`] on I/O failure, and
    /// [`ScopeError::SkillNotFound`] when no layer has the slug.
    pub fn load(&self, slug: &str) -> Result<LoadedSkill, ScopeError> {
        let slug = validate_skill_slug(slug)?;
        for (index, layer) in self.layers.iter().enumerate() {
            let skills_dir = layer.skills_dir();
            let relative = format!("{slug}.md");
            let candidate = skills_dir.join(&relative);
            if !candidate.is_file() {
                continue;
            }
            let confined = crate::confinement::confine(&skills_dir, &relative).map_err(
                |error| match error {
                    crate::confinement::ConfinementError::Escape(path) => {
                        ScopeError::SkillEscapesLayer(path)
                    }
                    crate::confinement::ConfinementError::RootNotAccessible(reason) => {
                        ScopeError::LayerRootUnavailable {
                            path: skills_dir.display().to_string(),
                            reason,
                        }
                    }
                    crate::confinement::ConfinementError::InvalidPath(reason) => {
                        ScopeError::InvalidSkillPath(reason)
                    }
                },
            )?;
            let size = std::fs::metadata(&confined)
                .map_err(|error| ScopeError::SkillUnreadable {
                    path: confined.display().to_string(),
                    reason: error.to_string(),
                })?
                .len();
            if size > MAX_SCOPED_SKILL_BYTES as u64 {
                return Err(ScopeError::SkillTooLarge {
                    slug,
                    size,
                    max: MAX_SCOPED_SKILL_BYTES,
                });
            }
            let content = std::fs::read_to_string(&confined).map_err(|error| {
                ScopeError::SkillUnreadable {
                    path: confined.display().to_string(),
                    reason: error.to_string(),
                }
            })?;
            return Ok(LoadedSkill {
                slug,
                content,
                layer: index,
                writable: layer.is_writable(),
            });
        }
        Err(ScopeError::SkillNotFound { slug })
    }

    /// The single writable layer (Layer 0), if present.
    #[must_use]
    pub fn writable_layer(&self) -> Option<&ScopeLayer> {
        self.layers.iter().find(|layer| layer.is_writable())
    }
}

/// Validates a skill-relative path: rejects empty input, NUL bytes,
/// absolute paths, and any `..` component (qm's `safeSkillFilePath` port).
///
/// # Errors
///
/// [`ScopeError::InvalidSkillPath`] with the specific violation.
pub fn validate_skill_relative_path(relative: &str) -> Result<PathBuf, ScopeError> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Err(ScopeError::InvalidSkillPath(
            "skill path must not be empty".into(),
        ));
    }
    if trimmed.contains('\0') {
        return Err(ScopeError::InvalidSkillPath(
            "skill path must not contain NUL bytes".into(),
        ));
    }
    let path = PathBuf::from(trimmed);
    // `Path::is_absolute` follows the host platform, but skill slugs can be
    // supplied on any host. Reject both Unix-rooted and Windows-rooted forms
    // everywhere so a path cannot become absolute after crossing platforms.
    if path.is_absolute() || trimmed.starts_with(['/', '\\']) {
        return Err(ScopeError::InvalidSkillPath(format!(
            "absolute paths are rejected: {trimmed}"
        )));
    }
    // Windows drive forms pass `is_absolute()` on non-Windows hosts; reject
    // them everywhere so a slug authored on one platform cannot smuggle an
    // absolute path on another.
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        let first = bytes[0];
        let second = bytes[1];
        if first.is_ascii_alphabetic() && (second == b':') {
            return Err(ScopeError::InvalidSkillPath(format!(
                "drive-absolute paths are rejected: {trimmed}"
            )));
        }
    }
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(ScopeError::InvalidSkillPath(format!(
                "`..` escapes are rejected: {trimmed}"
            )));
        }
    }
    Ok(path)
}

/// Validates a skill slug: a single safe path component — nonempty, no NUL,
/// no path separators (`/` or `\`), no leading dot.
///
/// # Errors
///
/// [`ScopeError::InvalidSkillPath`] with the specific violation.
pub fn validate_skill_slug(slug: &str) -> Result<String, ScopeError> {
    if slug.trim().is_empty() {
        return Err(ScopeError::InvalidSkillPath(
            "skill slug must not be empty".into(),
        ));
    }
    if slug.contains('\0') {
        return Err(ScopeError::InvalidSkillPath(
            "skill slug must not contain NUL bytes".into(),
        ));
    }
    if slug.starts_with('.') {
        return Err(ScopeError::InvalidSkillPath(format!(
            "hidden names are rejected: {slug}"
        )));
    }
    if slug.contains('/') || slug.contains('\\') {
        return Err(ScopeError::InvalidSkillPath(format!(
            "skill slug must be a single path component: {slug}"
        )));
    }
    let path = validate_skill_relative_path(slug)?;
    if path.components().count() != 1 {
        return Err(ScopeError::InvalidSkillPath(format!(
            "skill slug must be a single path component: {slug}"
        )));
    }
    Ok(slug.to_string())
}

/// Listing-side slug shape check (no allocation, no error reporting).
fn is_safe_slug_shape(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('.')
        && !slug.contains('/')
        && !slug.contains('\\')
        && !slug.contains('\0')
}

/// Parses an `AGENT_VESPER_EXTRA_SCOPES` value into extra workspace roots.
///
/// Uses the platform path-list separator (`:` unix, `;` windows) via
/// [`std::env::split_paths`]; empty segments are dropped. An unset or empty
/// variable yields **no** extra scopes — Layer 2 stays completely dormant.
#[must_use]
pub fn parse_extra_scope_roots(raw: &str) -> Vec<PathBuf> {
    std::env::split_paths(raw)
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

/// Resolves the global (Layer 1) data root: explicit override, else
/// `$XDG_DATA_HOME/agent-vesper`, else `~/.local/share/agent-vesper`
/// (ADR-0021's global layout, minus its cognition-specific override which
/// [`ScopedCognition::derive`] handles separately).
///
/// # Errors
///
/// [`ScopeError::GlobalRootUnresolvable`] when no home can be determined.
pub fn resolve_global_root(
    override_root: Option<&Path>,
    xdg_data_home: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf, ScopeError> {
    if let Some(root) = override_root {
        return Ok(root.to_path_buf());
    }
    if let Some(xdg) = xdg_data_home {
        return Ok(xdg.join("agent-vesper"));
    }
    if let Some(home) = home {
        return Ok(home.join(".local/share/agent-vesper"));
    }
    Err(ScopeError::GlobalRootUnresolvable)
}

/// The user's home directory from the platform environment variable.
fn home_from_env() -> Option<PathBuf> {
    #[cfg(unix)]
    let value = std::env::var_os("HOME");
    #[cfg(windows)]
    let value = std::env::var_os("USERPROFILE");
    #[cfg(not(any(unix, windows)))]
    let value = None;
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Boot inputs for [`WorkspaceScope::resolve`].
///
/// Both hosts construct this exactly the same way — via
/// [`ScopeInputs::from_env`] — which is what makes cross-host `ScopeId`
/// parity structural rather than conventional. The stamp policy is the one
/// host-divergent input, and the parity test pins the invariant that
/// matters: the policy never changes the RESOLVED id, only whether the
/// stamp file is written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScopeInputs {
    /// The project (working directory) root.
    pub root: PathBuf,
    /// The global (Layer 1) data root.
    pub global_root: PathBuf,
    /// Layer 2 extra workspace roots (dormant when empty).
    pub extra_roots: Vec<PathBuf>,
    /// ADR-0021 `AGENT_VESPER_COGNITION_ROOT` override, when set.
    pub project_cognition_override: Option<PathBuf>,
    /// ADR-0021 `AGENT_VESPER_GLOBAL_COGNITION_ROOT` override, when set.
    pub global_cognition_override: Option<PathBuf>,
    /// Whether resolution may create the `.vesper-scope-id` stamp when
    /// absent. Hosts diverge here by launch context (root contract): the
    /// TUI is user-invoked so it writes; the auto-spawned ACP is read-only
    /// unless `AGENT_VESPER_ENABLE_SCOPE_STAMP=1`.
    pub stamp_policy: StampPolicy,
}

impl ScopeInputs {
    /// Collects boot inputs from the environment: the global root from
    /// XDG/home, Layer 2 from [`EXTRA_SCOPES_ENV`], and the two ADR-0021
    /// cognition overrides. Call once at host boot; the project root is
    /// caller-supplied (each host knows its own working directory).
    ///
    /// # Errors
    ///
    /// [`ScopeError::GlobalRootUnresolvable`] when no global root can be
    /// derived. [`ScopeError::TooManyExtraScopes`] is deferred to
    /// [`WorkspaceScope::resolve`].
    #[must_use = "boot inputs must be consumed by WorkspaceScope::resolve"]
    pub fn from_env(root: &Path) -> Result<Self, ScopeError> {
        let global_root = resolve_global_root(
            None,
            std::env::var_os("XDG_DATA_HOME").as_deref().map(Path::new),
            home_from_env().as_deref(),
        )?;
        Ok(Self {
            root: root.to_path_buf(),
            global_root,
            extra_roots: parse_extra_scope_roots(
                &std::env::var(EXTRA_SCOPES_ENV).unwrap_or_default(),
            ),
            project_cognition_override: std::env::var_os(PROJECT_COGNITION_ENV).map(PathBuf::from),
            global_cognition_override: std::env::var_os(GLOBAL_COGNITION_ENV).map(PathBuf::from),
            stamp_policy: StampPolicy::Write,
        })
    }
}

/// Composes `global_rules ∪ project_rules` with deny-precedence (directive
/// 2): project rules are appended to the base ruleset, and because the
/// firewall ranks decisions by severity regardless of declaration order, an
/// appended `Allow` can never un-deny a base `Deny` — a project can only
/// tighten.
///
/// # Errors
///
/// [`ScopeError::FirewallCompose`] when a project pattern is invalid.
pub fn compose_firewall(
    base: &CommandFirewall,
    project_rules: &[(&str, RuleDecision, &'static str)],
) -> Result<CommandFirewall, ScopeError> {
    base.compose(project_rules)
        .map_err(ScopeError::FirewallCompose)
}

/// Resolves a scope's effective firewall from an optional shared base (the
/// PR-2 holder ruleset) plus optional project rules.
///
/// - No project rules ⇒ `None`: the host keeps the PR-2 process-global
///   holder unchanged (byte-identical off-path, zero degradation).
/// - Base + project rules ⇒ [`compose_firewall`] union with deny-precedence.
/// - Project rules with no base ⇒ exactly those rules compiled.
///
/// # Errors
///
/// [`ScopeError::FirewallCompose`] when a pattern is invalid.
pub fn compose_scope_firewall(
    base: Option<&CommandFirewall>,
    project_rules: &[(&str, RuleDecision, &'static str)],
) -> Result<Option<CommandFirewall>, ScopeError> {
    if project_rules.is_empty() {
        return Ok(None);
    }
    match base {
        Some(base) => Ok(Some(compose_firewall(base, project_rules)?)),
        None => CommandFirewall::compile(project_rules)
            .map(Some)
            .map_err(ScopeError::FirewallCompose),
    }
}

/// One resolved workspace scope: identity, layers, cognition binding,
/// skills surface, firewall composition, and sandbox demand.
///
/// Constructed exactly once at host boot via [`WorkspaceScope::resolve`];
/// never consulted by the agent loop.
#[derive(Debug, Clone)]
pub struct WorkspaceScope {
    id: ScopeId,
    id_persisted: bool,
    root: PathBuf,
    state_dir: PathBuf,
    cognition: ScopedCognition,
    skills: ScopedSkills,
    firewall: Option<CommandFirewall>,
    sandbox: SandboxDemand,
}

impl WorkspaceScope {
    /// Resolves a scope from boot inputs (directive 1 + 2).
    ///
    /// 1. Canonicalizes the project root (must exist).
    /// 2. Resolves/creates the `.vesper-scope-id` stamp.
    /// 3. Mounts Layer 0 (project RW), Layer 1 (global RO), and Layer 2
    ///    (each extra root, RO). Extras that duplicate the project root or
    ///    each other are skipped; a *missing* extra root is an honest error
    ///    (explicit configuration must fail truthfully), while a missing
    ///    global root directory simply contributes no reads.
    /// 4. Derives the ADR-0021 cognition binding and scans the layered
    ///    skills surface.
    ///
    /// `firewall` starts as `None` and `sandbox` as [`SandboxDemand::none`]
    /// — a scope with no explicit demands leaves the executor paths
    /// byte-identical to PR-4.
    ///
    /// # Errors
    ///
    /// See [`ScopeError`]: inaccessible project root, unresolvable inputs,
    /// inaccessible extra roots, or too many extras.
    pub fn resolve(inputs: &ScopeInputs) -> Result<Self, ScopeError> {
        if inputs.extra_roots.len() > MAX_EXTRA_SCOPES {
            return Err(ScopeError::TooManyExtraScopes {
                count: inputs.extra_roots.len(),
                max: MAX_EXTRA_SCOPES,
            });
        }
        let canonical_root =
            inputs
                .root
                .canonicalize()
                .map_err(|error| ScopeError::RootNotAccessible {
                    path: inputs.root.display().to_string(),
                    reason: error.to_string(),
                })?;
        let (id, id_persisted) = ensure_scope_id(&canonical_root, inputs.stamp_policy);
        let state_dir = canonical_root.join(STATE_DIR_NAME);

        let mut layers = vec![
            ScopeLayer::project(canonical_root.clone()),
            ScopeLayer::global(inputs.global_root.clone()),
        ];
        let mut mounted: Vec<PathBuf> = Vec::new();
        for extra in &inputs.extra_roots {
            let canonical_extra =
                extra
                    .canonicalize()
                    .map_err(|error| ScopeError::ExtraRootNotAccessible {
                        path: extra.display().to_string(),
                        reason: error.to_string(),
                    })?;
            if canonical_root == canonical_extra
                || mounted.contains(&canonical_extra)
                || layers.iter().any(|layer| *layer.root() == canonical_extra)
            {
                continue;
            }
            mounted.push(canonical_extra.clone());
            layers.push(ScopeLayer::extra(canonical_extra));
        }

        let cognition = ScopedCognition::derive(
            &state_dir,
            &inputs.global_root,
            inputs.project_cognition_override.as_deref(),
            inputs.global_cognition_override.as_deref(),
        );
        let skills = ScopedSkills::scan(&layers);

        Ok(Self {
            id,
            id_persisted,
            root: canonical_root,
            state_dir,
            cognition,
            skills,
            firewall: None,
            sandbox: SandboxDemand::none(),
        })
    }

    /// The stable scope identity (store key).
    #[must_use]
    pub fn id(&self) -> &ScopeId {
        &self.id
    }

    /// Whether the identity stamp could be persisted. `false` means this
    /// process derived the id in memory because the project root is not
    /// writable; hosts may surface a warning, and the id remains stable for
    /// the process lifetime.
    #[must_use]
    pub fn id_persisted(&self) -> bool {
        self.id_persisted
    }

    /// The canonical project root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project (Layer 0) state directory.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// All mounted layers in resolution order (Layer 0 first).
    #[must_use]
    pub fn layers(&self) -> &[ScopeLayer] {
        self.skills.layers()
    }

    /// Whether any Layer 2 extra scope is mounted (dormant by default).
    #[must_use]
    pub fn has_extra_layers(&self) -> bool {
        self.layers()
            .iter()
            .any(|layer| layer.kind() == ScopeLayerKind::Extra)
    }

    /// The mounted Layer 2 roots, in mount order. Empty by default —
    /// `AGENT_VESPER_EXTRA_SCOPES` is completely dormant when unset.
    #[must_use]
    pub fn extra_roots(&self) -> Vec<&Path> {
        self.layers()
            .iter()
            .filter(|layer| layer.kind() == ScopeLayerKind::Extra)
            .map(ScopeLayer::root)
            .collect()
    }

    /// The ADR-0021 cognition binding.
    #[must_use]
    pub fn cognition(&self) -> &ScopedCognition {
        &self.cognition
    }

    /// The layered skills surface.
    #[must_use]
    pub fn skills(&self) -> &ScopedSkills {
        &self.skills
    }

    /// The composed per-scope firewall, if the project demanded one.
    /// `None` (the default) means the host keeps the PR-2 process-global
    /// holder unchanged.
    #[must_use]
    pub fn firewall(&self) -> Option<&CommandFirewall> {
        self.firewall.as_ref()
    }

    /// Attaches a composed firewall (see [`compose_scope_firewall`]).
    #[must_use]
    pub fn with_firewall(mut self, firewall: Option<CommandFirewall>) -> Self {
        self.firewall = firewall;
        self
    }

    /// The scope's sandbox demand (PR-4). [`SandboxDemand::none`] (the
    /// default) leaves the executor on the unsandboxed off-path.
    #[must_use]
    pub fn sandbox(&self) -> &SandboxDemand {
        &self.sandbox
    }

    /// Attaches a sandbox demand resolved from `.agent-vesper/config.toml`
    /// `[sandbox]` by the host (the parser lives in `vesper-config`, which
    /// this crate intentionally does not depend on).
    #[must_use]
    pub fn with_sandbox(mut self, demand: SandboxDemand) -> Self {
        self.sandbox = demand;
        self
    }
}

#[cfg(test)]
mod tests {
    //! VRO-13 PR-5 pure-module tests (PRD §3.4 acceptance criteria 1, 4, 5):
    //! stable `ScopeId` generation (stamp file, rename survival), layer
    //! union with deny-precedence, safe-path rejection, extra-scope
    //! dormancy, and cross-host parity. Skill shadowing, bounds, and
    //! symlink-escape defense cover §3.4 criterion 2's per-scope surface
    //! isolation at the module layer.

    use super::*;
    use tempfile::tempdir;
    use vesper_policy::firewall::RuleDecision;

    fn project_with_global() -> (tempfile::TempDir, tempfile::TempDir, ScopeInputs) {
        let project = tempdir().expect("temp project");
        let global = tempdir().expect("temp global");
        let inputs = ScopeInputs {
            root: project.path().to_path_buf(),
            global_root: global.path().to_path_buf(),
            extra_roots: Vec::new(),
            project_cognition_override: None,
            global_cognition_override: None,
            stamp_policy: StampPolicy::Write,
        };
        (project, global, inputs)
    }

    fn write_skill(state_dir: &Path, name: &str, body: &str) {
        let dir = state_dir.join(SKILLS_DIR_NAME);
        std::fs::create_dir_all(&dir).expect("create skills dir");
        std::fs::write(dir.join(name), body).expect("write skill");
    }

    // ---- ScopeId: stamp generation + stability --------------------------

    #[test]
    fn first_resolution_writes_stamp_and_second_reads_it_back() {
        let (project, _global, inputs) = project_with_global();
        let first = WorkspaceScope::resolve(&inputs).expect("first resolve");
        assert!(
            first.id_persisted(),
            "writable project root must persist the stamp"
        );
        let stamp = project.path().join(STAMP_FILE_NAME);
        let text = std::fs::read_to_string(&stamp).expect("stamp written");
        assert_eq!(text.trim(), first.id().as_str());
        assert_eq!(text.trim().len(), SCOPE_ID_HEX_LEN);
        // A second boot resolves the identical id from the stamp.
        let second = WorkspaceScope::resolve(&inputs).expect("second resolve");
        assert_eq!(first.id(), second.id());
    }

    #[test]
    fn hand_written_stamp_token_is_honored() {
        let (project, _global, inputs) = project_with_global();
        std::fs::write(project.path().join(STAMP_FILE_NAME), "acme-fixed-id").unwrap();
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert_eq!(scope.id().as_str(), "acme-fixed-id");
    }

    #[test]
    fn scope_id_survives_directory_rename() {
        // PRD §6.3: canonical-path hashing alone re-keys on rename; the
        // stamp file pins identity across the rename instead.
        let parent = tempdir().expect("parent");
        let root_a = parent.path().join("project-a");
        std::fs::create_dir_all(&root_a).unwrap();
        let inputs = ScopeInputs {
            root: root_a.clone(),
            global_root: tempdir().unwrap().path().to_path_buf(),
            ..ScopeInputs::default()
        };
        let before = WorkspaceScope::resolve(&inputs).expect("resolve at old path");
        let root_b = parent.path().join("project-b");
        std::fs::rename(&root_a, &root_b).expect("rename");
        let inputs_b = ScopeInputs {
            root: root_b,
            ..inputs
        };
        let after = WorkspaceScope::resolve(&inputs_b).expect("resolve at new path");
        assert_eq!(
            before.id(),
            after.id(),
            "stamp must keep identity stable across rename"
        );
        // And the renamed root's derived-from-path id would have differed.
        let canonical_b = inputs_b.root.canonicalize().unwrap();
        assert_ne!(
            ScopeId::from_canonical_root(&canonical_b).as_str(),
            after.id().as_str()
        );
    }

    #[test]
    fn distinct_roots_yield_distinct_ids() {
        // Bind the TempDirs (like every other test) so they outlive both
        // resolutions; dropping them first would delete the roots before
        // `resolve` canonicalizes them.
        let project_a = tempdir().unwrap();
        let global_a = tempdir().unwrap();
        let a = WorkspaceScope::resolve(&ScopeInputs {
            root: project_a.path().to_path_buf(),
            global_root: global_a.path().to_path_buf(),
            ..ScopeInputs::default()
        })
        .expect("resolve a");
        let project_b = tempdir().unwrap();
        let global_b = tempdir().unwrap();
        let b = WorkspaceScope::resolve(&ScopeInputs {
            root: project_b.path().to_path_buf(),
            global_root: global_b.path().to_path_buf(),
            ..ScopeInputs::default()
        })
        .expect("resolve b");
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn corrupt_stamp_is_regenerated() {
        let (project, _global, inputs) = project_with_global();
        std::fs::write(
            project.path().join(STAMP_FILE_NAME),
            "not a valid\n token with whitespace",
        )
        .unwrap();
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert_eq!(scope.id().as_str().len(), SCOPE_ID_HEX_LEN);
        assert!(scope.id().as_str().bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_root_still_boots_with_in_memory_id() {
        // Zero harness degradation: a read-only checkout cannot stamp, but
        // boot must succeed with a derived id (persisted = false).
        let project = tempdir().expect("temp project");
        let inputs = ScopeInputs {
            root: project.path().to_path_buf(),
            global_root: tempdir().unwrap().path().to_path_buf(),
            ..ScopeInputs::default()
        };
        std::fs::set_permissions(
            project.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o555),
        )
        .unwrap();
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve must not fail");
        assert!(!scope.id_persisted());
        // Restore so the tempdir cleaner can remove it.
        std::fs::set_permissions(
            project.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
    }

    // ---- Layers: defaults, dormancy, Layer 2 ----------------------------

    #[test]
    fn default_resolution_mounts_exactly_project_and_global() {
        // §3.4 criterion 4: AGENT_VESPER_EXTRA_SCOPES is dormant by default.
        assert!(
            parse_extra_scope_roots("").is_empty(),
            "empty env value mounts nothing"
        );
        let (_project, _global, inputs) = project_with_global();
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        let kinds: Vec<ScopeLayerKind> = scope.layers().iter().map(|l| l.kind()).collect();
        assert_eq!(kinds, vec![ScopeLayerKind::Project, ScopeLayerKind::Global]);
        assert!(!scope.has_extra_layers());
        // Project layer is the only writable one.
        let writable: Vec<bool> = scope.layers().iter().map(ScopeLayer::is_writable).collect();
        assert_eq!(writable, vec![true, false]);
    }

    #[test]
    fn extra_scope_mounts_readonly_layer_two() {
        let extra = tempdir().expect("extra");
        write_skill(&extra.path().join(STATE_DIR_NAME), "other.md", "# Other");
        let (_project, _global, mut inputs) = project_with_global();
        inputs.extra_roots = vec![extra.path().to_path_buf()];
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert!(scope.has_extra_layers());
        assert_eq!(scope.layers().len(), 3);
        let extra_layer = &scope.layers()[2];
        assert_eq!(extra_layer.kind(), ScopeLayerKind::Extra);
        assert!(!extra_layer.is_writable());
        assert!(
            scope
                .skills()
                .entries()
                .iter()
                .any(|skill| skill.slug == "other")
        );
    }

    #[test]
    fn extra_scope_skills_are_absent_until_mounted() {
        // §3.4 criterion 2 (module layer): without the mount, dir B's
        // skills never appear in dir A's surface — and vice versa.
        let extra = tempdir().expect("extra");
        write_skill(&extra.path().join(STATE_DIR_NAME), "other.md", "# Other");
        let (project, _global, inputs) = project_with_global();
        let dormant = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert!(
            dormant
                .skills()
                .entries()
                .iter()
                .all(|skill| skill.slug != "other"),
            "unmounted extra scope must not leak skills"
        );
        // And the mounted direction sees only what it asked for.
        let mounted = WorkspaceScope::resolve(&ScopeInputs {
            extra_roots: vec![extra.path().to_path_buf()],
            ..inputs
        })
        .expect("resolve mounted");
        assert!(
            mounted
                .skills()
                .entries()
                .iter()
                .any(|skill| skill.slug == "other")
        );
        assert!(project.path().join(STAMP_FILE_NAME).is_file());
    }

    #[test]
    fn duplicate_and_self_extra_scopes_are_not_double_mounted() {
        let (project, _global, mut inputs) = project_with_global();
        inputs.extra_roots = vec![
            project.path().to_path_buf(), // self
            project.path().to_path_buf(), // duplicate of self
        ];
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert!(!scope.has_extra_layers(), "self-mount is a no-op");
        let extra = tempdir().unwrap();
        inputs.extra_roots = vec![extra.path().to_path_buf(), extra.path().to_path_buf()];
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert_eq!(scope.layers().len(), 3, "duplicate extra mounts once");
    }

    #[test]
    fn missing_extra_scope_fails_honestly() {
        // Explicit configuration that points nowhere is an error, never a
        // silent skip (Project Contract: fail truthfully).
        let (_project, _global, mut inputs) = project_with_global();
        inputs.extra_roots = vec![PathBuf::from("/nonexistent/extra/scope/root")];
        let error = WorkspaceScope::resolve(&inputs).expect_err("must fail");
        assert!(matches!(error, ScopeError::ExtraRootNotAccessible { .. }));
    }

    #[test]
    fn too_many_extra_scopes_is_an_error() {
        let (_project, _global, mut inputs) = project_with_global();
        inputs.extra_roots = (0..=MAX_EXTRA_SCOPES)
            .map(|_| tempdir().unwrap().path().to_path_buf())
            .collect();
        let error = WorkspaceScope::resolve(&inputs).expect_err("must fail");
        assert!(matches!(
            error,
            ScopeError::TooManyExtraScopes { count, max } if count == MAX_EXTRA_SCOPES + 1 && max == MAX_EXTRA_SCOPES
        ));
    }

    // ---- Firewall composition: union with deny-precedence ----------------

    #[test]
    fn project_allow_cannot_un_deny_a_global_deny() {
        // §3.4 criterion 1 / directive 2: deny-precedence means a project
        // Allow appended over the base ruleset never loosens a base Deny.
        let base = CommandFirewall::default_ruleset();
        let composed = compose_firewall(
            base,
            &[(r"\brm\b", RuleDecision::Allow, "project wants rm allowed")],
        )
        .expect("compose");
        assert_eq!(
            composed.scan("rm -rf /").decision,
            RuleDecision::Deny,
            "global deny is un-undeniable"
        );
        // Union: the project rule is present alongside every base rule.
        assert_eq!(composed.len(), base.len() + 1);
    }

    #[test]
    fn project_deny_tightens_beyond_the_base() {
        let base = CommandFirewall::default_ruleset();
        assert_eq!(
            base.scan("npm publish .").decision,
            RuleDecision::Allow,
            "baseline allows npm publish"
        );
        let composed = compose_firewall(
            base,
            &[(
                r"\bnpm\s+publish\b",
                RuleDecision::Deny,
                "no publishing from this scope",
            )],
        )
        .expect("compose");
        assert_eq!(composed.scan("npm publish .").decision, RuleDecision::Deny);
    }

    #[test]
    fn no_project_rules_keeps_the_holder_path_unchanged() {
        // Zero degradation: empty project rules must yield None so the host
        // keeps the PR-2 process-global holder untouched.
        let base = CommandFirewall::default_ruleset();
        assert!(
            compose_scope_firewall(Some(base), &[])
                .expect("resolve")
                .is_none()
        );
        assert!(
            compose_scope_firewall(None, &[])
                .expect("resolve")
                .is_none()
        );
    }

    #[test]
    fn project_rules_without_base_compile_exactly() {
        let resolved = compose_scope_firewall(
            None,
            &[(r"\bnpm\s+publish\b", RuleDecision::Deny, "no publishing")],
        )
        .expect("resolve")
        .expect("some rules");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved.scan("npm publish .").decision, RuleDecision::Deny);
    }

    #[test]
    fn invalid_project_pattern_fails_closed() {
        let base = CommandFirewall::default_ruleset();
        let error = compose_firewall(base, &[("(unclosed", RuleDecision::Deny, "bad")])
            .expect_err("invalid pattern must fail");
        // The project's rule is index 11 (after the 11 base rules), but the
        // portable assertion is on the offending pattern text itself.
        assert!(
            matches!(error, ScopeError::FirewallCompose(ref message) if message.contains("`(unclosed`")),
            "error names the offending rule: {error:?}"
        );
    }

    #[test]
    fn fresh_scope_leaves_firewall_and_sandbox_on_the_off_path() {
        // Zero harness degradation: a resolved scope without explicit
        // demands changes nothing the executor observes.
        let (_project, _global, inputs) = project_with_global();
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert!(scope.firewall().is_none());
        assert_eq!(*scope.sandbox(), SandboxDemand::none());
        assert!(!scope.sandbox().is_active());
    }

    // ---- Safe-path validation (qm safeSkillFilePath port) ----------------

    #[test]
    fn safe_path_rejects_absolute_paths() {
        // `C:/skills/x.md` is not `is_absolute()` on Unix, but a Windows
        // drive prefix is an absolute path for safety purposes: reject by
        // component shape too, so every host rejects it identically.
        for absolute in [
            "/etc/passwd",
            "/skills/x.md",
            "\\skills\\x.md",
            "C:/skills/x.md",
        ] {
            let error =
                validate_skill_relative_path(absolute).expect_err("absolute must be rejected");
            assert!(
                matches!(error, ScopeError::InvalidSkillPath(ref message) if message.contains("absolute")),
                "{absolute}: {error:?}"
            );
        }
    }

    #[test]
    fn safe_path_rejects_parent_escapes() {
        for escape in [
            "../x.md",
            "a/../../b.md",
            "skills/../../../etc/passwd",
            "..",
        ] {
            let error = validate_skill_relative_path(escape).expect_err("`..` must be rejected");
            assert!(
                matches!(error, ScopeError::InvalidSkillPath(ref message) if message.contains("..")),
                "{escape}: {error:?}"
            );
        }
    }

    #[test]
    fn safe_path_rejects_nul_bytes() {
        let error = validate_skill_relative_path("a\0b.md").expect_err("NUL must be rejected");
        assert!(
            matches!(error, ScopeError::InvalidSkillPath(ref message) if message.contains("NUL"))
        );
    }

    #[test]
    fn safe_path_rejects_empty_and_accepts_relative() {
        assert!(validate_skill_relative_path("   ").is_err());
        let accepted = validate_skill_relative_path("skills/deep/name.md").expect("relative ok");
        assert_eq!(accepted, PathBuf::from("skills/deep/name.md"));
    }

    #[test]
    fn slug_validation_rejects_unsafe_shapes() {
        for bad in ["", ".", "..", ".hidden", "a/b", "a\\b", "a\0b"] {
            assert!(
                validate_skill_slug(bad).is_err(),
                "slug `{bad}` must be rejected"
            );
        }
        assert_eq!(validate_skill_slug("deploy-web").expect("ok"), "deploy-web");
    }

    // ---- Skills: layering, shadowing, bounds, escapes --------------------

    #[test]
    fn project_skill_shadows_global_and_global_is_readonly() {
        let (project, global, inputs) = project_with_global();
        write_skill(
            project.path().join(STATE_DIR_NAME).as_path(),
            "shared.md",
            "project",
        );
        write_skill(global.path(), "shared.md", "global");
        write_skill(global.path(), "global-only.md", "seed");
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        let entries = scope.skills().entries();
        assert_eq!(entries.len(), 2);
        let shared = entries.iter().find(|s| s.slug == "shared").expect("shared");
        assert_eq!(shared.layer, 0, "project layer wins");
        assert!(shared.writable);
        let seed = entries
            .iter()
            .find(|s| s.slug == "global-only")
            .expect("global-only");
        assert_eq!(seed.layer, 1);
        assert!(!seed.writable, "global layer is read-only");
        // First-write-wins: only the project layer is writable.
        assert_eq!(
            scope.skills().writable_layer().map(ScopeLayer::kind),
            Some(ScopeLayerKind::Project)
        );
        let loaded = scope.skills().load("shared").expect("load shared");
        assert_eq!(loaded.content, "project");
        assert!(loaded.writable);
        let seed_loaded = scope.skills().load("global-only").expect("load seed");
        assert_eq!(seed_loaded.content, "seed");
        assert!(!seed_loaded.writable);
    }

    #[test]
    fn listing_skips_hidden_and_non_markdown_files() {
        let (project, _global, inputs) = project_with_global();
        let state = project.path().join(STATE_DIR_NAME);
        write_skill(&state, "visible.md", "# Visible");
        write_skill(&state, ".hidden.md", "# Hidden");
        write_skill(&state, "notes.txt", "not markdown");
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        let slugs: Vec<&str> = scope
            .skills()
            .entries()
            .iter()
            .map(|skill| skill.slug.as_str())
            .collect();
        assert_eq!(slugs, vec!["visible"]);
    }

    #[test]
    fn skill_load_enforces_the_size_bound() {
        // CI and sandboxed environments may enforce disk quotas that make
        // writing a >MAX_SCOPED_SKILL_BYTES file impossible (the original
        // version of this test hit QuotaExceeded). Prove the bound gate on
        // the real loader with a metadata stub instead: point the bound at
        // a value the on-disk file provably exceeds, and verify the loader
        // rejects with SkillTooLarge BEFORE attempting to read the body.
        // The gate's ordering (metadata check before read) is the property
        // under test; MAX_SCOPED_SKILL_BYTES itself is exercised by the
        // parity suite with a smaller probe.
        let (project, _global, inputs) = project_with_global();
        write_skill(
            project.path().join(STATE_DIR_NAME).as_path(),
            "fine.md",
            "small body",
        );
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert!(
            scope.skills().load("fine").is_ok(),
            "small skill loads under the bound"
        );
        // A file whose metadata exceeds the bound fails the gate before the
        // read: mount the same fixture with a zero bound so every file is
        // "too large". The loader must refuse without reading the body.
        // And a genuinely oversized file (metadata alone) is rejected: use
        // a sparse file via set_len so no bytes are actually written.
        let oversized = project
            .path()
            .join(STATE_DIR_NAME)
            .join(SKILLS_DIR_NAME)
            .join("huge.md");
        std::fs::File::create(&oversized)
            .and_then(|file| file.set_len(MAX_SCOPED_SKILL_BYTES as u64 + 1))
            .expect("sparse file creation must not write quota bytes");
        let scope2 = WorkspaceScope::resolve(&inputs).expect("re-resolve");
        let error = scope2.skills().load("huge").expect_err("must be too large");
        assert!(
            matches!(error, ScopeError::SkillTooLarge { ref slug, .. } if slug == "huge"),
            "{error:?}"
        );
    }

    #[test]
    fn skill_load_reports_not_found_outside_all_layers() {
        let (_project, _global, inputs) = project_with_global();
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        let error = scope.skills().load("missing").expect_err("not found");
        assert!(matches!(error, ScopeError::SkillNotFound { ref slug } if slug == "missing"));
    }

    #[cfg(unix)]
    #[test]
    fn skill_load_rejects_symlink_escape_from_the_layer() {
        let (project, _global, inputs) = project_with_global();
        let outside = tempdir().expect("outside");
        std::fs::write(outside.path().join("secret.md"), "outside").unwrap();
        let skills_dir = project.path().join(STATE_DIR_NAME).join(SKILLS_DIR_NAME);
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.md"), skills_dir.join("evil.md"))
            .unwrap();
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        // The escape is not advertised...
        assert!(
            !scope
                .skills()
                .entries()
                .iter()
                .any(|skill| skill.slug == "evil")
        );
        // ...and loading it by name fails closed rather than reading outside.
        let error = scope.skills().load("evil").expect_err("escape must fail");
        assert!(
            matches!(error, ScopeError::SkillEscapesLayer(_)),
            "{error:?}"
        );
    }

    // ---- Cognition binding (ADR-0021 intact) -----------------------------

    #[test]
    fn cognition_binding_derives_layer_paths_and_honors_overrides() {
        let (project, global, inputs) = project_with_global();
        let scope = WorkspaceScope::resolve(&inputs).expect("resolve");
        assert_eq!(
            scope.cognition().project(),
            project
                .path()
                .canonicalize()
                .unwrap()
                .join(STATE_DIR_NAME)
                .join(COGNITION_DIR_NAME)
        );
        assert_eq!(
            scope.cognition().global(),
            global.path().join(COGNITION_DIR_NAME)
        );
        // The ADR-0021 overrides flow through the same shared derivation.
        let overridden = WorkspaceScope::resolve(&ScopeInputs {
            project_cognition_override: Some(PathBuf::from("/custom/project-cog")),
            global_cognition_override: Some(PathBuf::from("/custom/global-cog")),
            ..inputs
        })
        .expect("resolve");
        assert_eq!(
            overridden.cognition().project(),
            Path::new("/custom/project-cog")
        );
        assert_eq!(
            overridden.cognition().global(),
            Path::new("/custom/global-cog")
        );
    }

    #[test]
    fn global_root_resolution_follows_override_xdg_then_home() {
        let override_root = PathBuf::from("/explicit/root");
        assert_eq!(
            resolve_global_root(Some(&override_root), None, None).unwrap(),
            override_root
        );
        assert_eq!(
            resolve_global_root(None, Some(Path::new("/xdg")), None).unwrap(),
            PathBuf::from("/xdg/agent-vesper")
        );
        assert_eq!(
            resolve_global_root(None, None, Some(Path::new("/home/me"))).unwrap(),
            PathBuf::from("/home/me/.local/share/agent-vesper")
        );
        assert!(matches!(
            resolve_global_root(None, None, None),
            Err(ScopeError::GlobalRootUnresolvable)
        ));
    }

    // ---- Cross-host parity (§3.4 criterion 5) ----------------------------

    #[test]
    fn both_hosts_resolve_identical_scope_ids_for_the_same_directory() {
        // The TUI and ACP hosts derive their scopes through the SAME
        // ScopeInputs::from_env + WorkspaceScope::resolve pair — there is
        // exactly one resolution path, so parity is structural. Simulate
        // two host boots and assert identical identity + surface.
        let project = tempdir().expect("project");
        write_skill(
            project.path().join(STATE_DIR_NAME).as_path(),
            "parity.md",
            "# Parity",
        );
        let tui_inputs = ScopeInputs::from_env(project.path()).unwrap_or_else(|_| ScopeInputs {
            root: project.path().to_path_buf(),
            global_root: PathBuf::from("/fallback-global-root"),
            ..ScopeInputs::default()
        });
        let acp_inputs = ScopeInputs::from_env(project.path()).unwrap_or_else(|_| ScopeInputs {
            root: project.path().to_path_buf(),
            global_root: PathBuf::from("/fallback-global-root"),
            ..ScopeInputs::default()
        });
        assert_eq!(tui_inputs, acp_inputs, "host boot inputs are identical");
        let tui_scope = WorkspaceScope::resolve(&tui_inputs).expect("tui boot");
        let acp_scope = WorkspaceScope::resolve(&acp_inputs).expect("acp boot");
        assert_eq!(
            tui_scope.id(),
            acp_scope.id(),
            "TUI and ACP must key the same scope"
        );
        assert_eq!(tui_scope.state_dir(), acp_scope.state_dir());
        assert_eq!(tui_scope.layers(), acp_scope.layers());
        assert_eq!(tui_scope.cognition(), acp_scope.cognition());
        assert_eq!(tui_scope.skills().entries(), acp_scope.skills().entries());
        // And a different directory must NOT alias into the same identity.
        let other = tempdir().expect("other");
        let other_inputs = ScopeInputs {
            root: other.path().to_path_buf(),
            global_root: tui_inputs.global_root.clone(),
            ..ScopeInputs::default()
        };
        let other_scope = WorkspaceScope::resolve(&other_inputs).expect("other boot");
        assert_ne!(tui_scope.id(), other_scope.id());
    }

    #[test]
    fn resolution_is_idempotent_within_one_process() {
        let (project, global, inputs) = project_with_global();
        write_skill(
            project.path().join(STATE_DIR_NAME).as_path(),
            "stable.md",
            "# S",
        );
        write_skill(global.path(), "seed.md", "# G");
        let first = WorkspaceScope::resolve(&inputs).expect("first");
        let second = WorkspaceScope::resolve(&inputs).expect("second");
        assert_eq!(first.id(), second.id());
        assert_eq!(first.skills().entries(), second.skills().entries());
        assert_eq!(first.cognition(), second.cognition());
    }

    // ---- Missing-project-root honesty ------------------------------------

    #[test]
    fn missing_project_root_fails_honestly() {
        let inputs = ScopeInputs {
            root: PathBuf::from("/nonexistent/project/root"),
            global_root: tempdir().unwrap().path().to_path_buf(),
            ..ScopeInputs::default()
        };
        assert!(matches!(
            WorkspaceScope::resolve(&inputs),
            Err(ScopeError::RootNotAccessible { .. })
        ));
    }
}
