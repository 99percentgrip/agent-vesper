//! Host-resolved workspace scope adapter (VRO-13 PR-5).
//!
//! [`holder::install_from_env`] resolves a [`WorkspaceScope`] for the
//! process's current working directory exactly once at host boot — the same
//! one-process, first-resolution-wins contract as the PR-2 firewall holder
//! (`vesper_policy::firewall::holder`) and the PR-4 sandbox holder. Both
//! hosts (TUI and ACP) call it in `main()` before any session exists, which
//! makes "TUI and ACP derive identical ScopeIds for the same directory" a
//! structural property: there is one resolution function and both hosts
//! call it, rather than two host-local copies that could drift.
//!
//! The holder stores only what hosts need post-boot: the scope id, the
//! project root, the state dir, the scoped skill catalog, and the cognition
//! roots. It performs no I/O after boot, is never consulted mid-loop, and
//! never mutates after installation: a scope change requires a host
//! restart (mirroring the firewall and sandbox holders).
//!
//! Cognition binding (ADR-0021) stays owned by the hosts: the holder
//! exposes the derived roots via [`ResolvedScope::cognition`], and each
//! host opens its own engines from those paths. No engine, store, or
//! routing decision lives here.
//!
//! The holder is `Mutex`-backed (not `OnceLock`) because `OnceLock` cannot
//! be cleared, and the parity tests in `tests/scope_parity.rs` must reset
//! the process-global state between cases. Production callers only ever
//! install once at boot, so the mutex is uncontended in practice.
//!
//! # Test serialization
//!
//! `SCOPE_HOLDER` is a process global and the in-crate tests reset it, so
//! every test that touches the holder must hold [`TEST_MUTEX`] for its
//! whole body. Parallel threads otherwise interleave reset/install pairs
//! across tests and produce spurious "two ids for one directory" failures
//! (`holder_starts_empty_and_resolves_once` was exactly this flake). The
//! integration tests in `tests/scope_parity.rs` already follow this
//! discipline and pin `--test-threads=1` from outside.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use vesper_agent::vro::scope::{
    ScopeError, ScopeId, ScopeInputs, ScopedCognition, ScopedSkills, StampPolicy,
};

/// Process-global resolved scope, installed once at host boot.
static SCOPE_HOLDER: Mutex<Option<Arc<ResolvedScope>>> = Mutex::new(None);

/// The boot-resolved scope snapshot hosts consume.
#[derive(Debug)]
pub struct ResolvedScope {
    id: ScopeId,
    root: PathBuf,
    state_dir: PathBuf,
    skills: ScopedSkills,
    cognition: ScopedCognition,
}

impl ResolvedScope {
    /// The stable scope identity (stamp-file pinned).
    #[must_use]
    pub fn id(&self) -> &ScopeId {
        &self.id
    }

    /// The project root this scope was resolved from.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project state directory (Layer 0, read-write).
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// The per-scope skill catalog resolved at boot.
    #[must_use]
    pub fn skills(&self) -> &ScopedSkills {
        &self.skills
    }

    /// The derived cognition roots (ADR-0021 binding).
    #[must_use]
    pub fn cognition(&self) -> &ScopedCognition {
        &self.cognition
    }
}

/// One-process scope resolution. See the module docs for the boot contract.
pub mod holder {
    use super::*;

    /// Resolves the workspace scope for the current working directory.
    ///
    /// First resolution wins and is immutable for the process lifetime,
    /// mirroring the firewall and sandbox holders. Errors do not install a
    /// scope (the holder stays `None`): hosts surface the error and boot
    /// without scope-keyed surfaces rather than crashing, the same graceful
    /// degradation an unreadable config would get.
    ///
    /// # Errors
    ///
    /// The [`ScopeError`] from [`WorkspaceScope::resolve`] (unresolvable
    /// global root, inaccessible project root, bad extra scopes, …) when
    /// this is the first resolution and it fails; `Ok(None)` on later
    /// calls when no scope was ever installed.
    pub fn install_from_env() -> Result<Option<Arc<ResolvedScope>>, ScopeError> {
        install_from_env_with_stamp_policy(StampPolicy::Write)
    }

    /// Resolves once using an explicit stamp-persistence policy.
    ///
    /// The ACP composition uses [`StampPolicy::ReadOnly`] by default so a
    /// process auto-spawned by an editor cannot create durable project state.
    /// The user-invoked TUI uses the default writing policy above.
    pub fn install_from_env_with_stamp_policy(
        stamp_policy: StampPolicy,
    ) -> Result<Option<Arc<ResolvedScope>>, ScopeError> {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        install_for_root_with_stamp_policy(&root, stamp_policy)
    }

    /// Resolves the scope for an explicit root. The ACP host's session-entry
    /// seam (a session may be rooted elsewhere than the process CWD) and
    /// the test entry point.
    ///
    /// # Errors
    ///
    /// The [`ScopeError`] from resolution when this is the first
    /// resolution; `Ok(None)` when a scope was already installed (first
    /// resolution wins) or the holder is empty.
    pub fn install_for_root(root: &Path) -> Result<Option<Arc<ResolvedScope>>, ScopeError> {
        install_for_root_with_stamp_policy(root, StampPolicy::Write)
    }

    /// Testable explicit-policy variant of [`install_for_root`].
    pub fn install_for_root_with_stamp_policy(
        root: &Path,
        stamp_policy: StampPolicy,
    ) -> Result<Option<Arc<ResolvedScope>>, ScopeError> {
        let mut guard = SCOPE_HOLDER
            .lock()
            .map_err(|_| ScopeError::HolderPoisoned)?;
        if let Some(existing) = guard.as_ref() {
            return Ok(Some(Arc::clone(existing)));
        }
        let mut inputs = ScopeInputs::from_env(root)?;
        inputs.stamp_policy = stamp_policy;
        let scope = vesper_agent::vro::scope::WorkspaceScope::resolve(&inputs)?;
        let resolved = Arc::new(ResolvedScope {
            id: scope.id().clone(),
            root: scope.root().to_path_buf(),
            state_dir: scope.state_dir().to_path_buf(),
            skills: scope.skills().clone(),
            cognition: scope.cognition().clone(),
        });
        *guard = Some(Arc::clone(&resolved));
        Ok(Some(resolved))
    }

    /// The installed scope, when one was resolved at boot.
    #[must_use]
    pub fn shared() -> Option<Arc<ResolvedScope>> {
        SCOPE_HOLDER.lock().ok().and_then(|guard| guard.clone())
    }

    /// Stable identity of the installed scope for cross-host parity
    /// assertions: the scope id string. Two hosts resolving the same
    /// directory share the id; distinct directories do not.
    #[must_use]
    pub fn scope_id() -> Option<ScopeId> {
        shared().map(|scope| scope.id().clone())
    }

    /// Clears the holder. **Test-only**: parity tests reset the
    /// process-global state between cases. Production code must never call
    /// this — a scope change requires a host restart.
    #[cfg(test)]
    pub fn reset_for_tests() {
        if let Ok(mut guard) = SCOPE_HOLDER.lock() {
            *guard = None;
        }
    }

    /// Clears the holder. **Test-only**, integration-test variant: exposes
    /// the reset to the `scope_parity.rs` binary without leaking it into
    /// production builds.
    #[cfg(not(test))]
    #[doc(hidden)]
    pub fn reset_for_tests() {
        if let Ok(mut guard) = SCOPE_HOLDER.lock() {
            *guard = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes every holder test for its whole body: the holder is
    /// process-global and tests reset it, so parallel execution interleaves
    /// reset/install pairs and fails spuriously.
    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn serialized() -> std::sync::MutexGuard<'static, ()> {
        TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn holder_starts_empty_and_resolves_once() {
        let _lock = serialized();
        holder::reset_for_tests();
        let root = tempfile::tempdir().expect("project root");
        let first = holder::install_for_root(root.path()).expect("first resolution installs");
        assert!(first.is_some(), "CWD resolution must succeed");
        let first_id = first.as_ref().map(|scope| scope.id().clone());
        let second = holder::install_for_root(root.path()).expect("second resolution agrees");
        assert_eq!(
            first_id,
            second.map(|scope| scope.id().clone()),
            "repeated resolutions return the same installed instance"
        );
        assert_eq!(
            holder::scope_id(),
            first_id,
            "scope_id matches the installed instance"
        );
        holder::reset_for_tests();
    }

    #[test]
    fn holder_is_first_resolution_wins() {
        let _lock = serialized();
        holder::reset_for_tests();
        let first_root = tempfile::tempdir().expect("first");
        let second_root = tempfile::tempdir().expect("second");
        let first = holder::install_for_root(first_root.path())
            .expect("first resolves")
            .expect("installed");
        let later = holder::install_for_root(second_root.path())
            .expect("later resolution does not fail")
            .expect("installed");
        assert_eq!(
            first.id(),
            later.id(),
            "later installs observe the first-resolved scope, not their own"
        );
        holder::reset_for_tests();
    }

    #[test]
    fn read_only_stamp_policy_never_creates_project_state() {
        let _lock = serialized();
        holder::reset_for_tests();
        let root = tempfile::tempdir().expect("project root");
        let resolved =
            holder::install_for_root_with_stamp_policy(root.path(), StampPolicy::ReadOnly)
                .expect("read-only resolution succeeds")
                .expect("scope installs");
        assert!(!resolved.id().as_str().is_empty());
        assert!(
            !root.path().join(".vesper-scope-id").exists(),
            "ACP default must not create a project stamp"
        );
        holder::reset_for_tests();
    }

    #[test]
    fn failed_first_resolution_does_not_install() {
        let _lock = serialized();
        holder::reset_for_tests();
        let error = holder::install_for_root(Path::new("/nonexistent/project/root"))
            .expect_err("inaccessible root must fail honestly");
        assert!(matches!(error, ScopeError::RootNotAccessible { .. }));
        assert!(
            holder::shared().is_none(),
            "a failed first resolution must not install anything"
        );
        holder::reset_for_tests();
    }
}
