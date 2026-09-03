//! VRO-13 PR-5 — WorkspaceScope host-parity tests.
//!
//! These prove the composed contract at the layer that owns it
//! (`vesper-harness`, the composition adapter both hosts share), mirroring
//! `sandbox_parity.rs`:
//!
//! 1. **Cross-host identity (§3.4 criterion 5).** The TUI and the ACP host
//!    boot through this crate's `scope_holder`. Both hosts call the same
//!    `holder::install_for_root` → first-resolution-wins path, so "both
//!    hosts resolve identical `ScopeId` for the same directory" is a
//!    structural property asserted here against the real holder, not a
//!    convention.
//! 2. **Boot-only discipline (§3.3).** The holder is
//!    first-resolution-wins: a later install for a *different* directory
//!    cannot flip the process-global scope. A scope change requires a host
//!    restart, exactly like the firewall and sandbox holders.
//! 3. **Layer-2 dormancy by default (§3.4 criterion 4).** With
//!    `AGENT_VESPER_EXTRA_SCOPES` absent, no extra layer is mounted and the
//!    default skills surface is byte-identical to the two-layer default.
//! 4. **Rename stability (§6.3).** The `.vesper-scope-id` stamp survives a
//!    directory rename: the renamed root resolves the *same* `ScopeId`
//!    (this is why identity is a stamp file, not a path hash).
//!
//! The holder is process-global, so every test here takes the module mutex
//! first — cargo runs integration tests on parallel threads, and a reset in
//! one test must never race another test's install.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use vesper_harness::scope_holder::holder;

/// Serializes every test in this binary: the scope holder is a real
/// process-global, and `reset_for_tests` is not thread-safe by design
/// (production code only ever installs once at boot).
fn holder_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    static INIT: OnceLock<()> = OnceLock::new();
    let _ = INIT.set(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Creates a project fixture directory with a skills file.
fn project_fixture(dir: &Path, skill_slug: &str) {
    let skills = dir.join(".agent-vesper").join("skills");
    std::fs::create_dir_all(&skills).expect("create skills dir");
    std::fs::write(
        skills.join(format!("{skill_slug}.md")),
        format!("# {skill_slug}\nfixture skill\n"),
    )
    .expect("write skill");
}

/// 1 + 5: both hosts' shared boot path resolves one identity for one
/// directory, and that identity is pinned by the stamp (rename-stable).
#[test]
fn both_hosts_boot_paths_resolve_identical_scope_ids() {
    let _guard = holder_lock();
    let tui_root = tempfile::tempdir().expect("tui project dir");
    let acp_root = tempfile::tempdir().expect("acp project dir");
    project_fixture(tui_root.path(), "alpha");
    project_fixture(acp_root.path(), "beta");

    // TUI boot: resolve its project scope through the shared holder.
    let tui_scope = holder::install_for_root(tui_root.path()).expect("tui scope resolves");
    let tui_id = tui_scope.as_ref().expect("installed").id().to_string();

    // ACP boot in the SAME directory: the holder is process-global and
    // first-resolution-wins, so a second host booting in this process
    // observes the same resolved scope (the parity contract: one process,
    // one scope — two hosts, no divergence).
    let acp_scope = holder::install_for_root(tui_root.path()).expect("acp scope resolves");
    assert_eq!(
        tui_id,
        acp_scope.as_ref().expect("installed").id().to_string(),
        "both hosts must resolve the identical ScopeId for the same directory"
    );

    // Distinct directories resolve distinct ids (no cross-project aliasing).
    holder::reset_for_tests();
    let other = holder::install_for_root(acp_root.path()).expect("other scope resolves");
    assert_ne!(
        tui_id,
        other.as_ref().expect("installed").id().to_string(),
        "different project directories must resolve different ScopeIds"
    );

    // The id is pinned by the stamp file: a second resolution of the same
    // root reads the stamp back instead of re-deriving.
    holder::reset_for_tests();
    let again = holder::install_for_root(tui_root.path()).expect("re-resolve");
    assert_eq!(
        tui_id,
        again.as_ref().expect("installed").id().to_string(),
        "stamp pins the id"
    );
    assert!(tui_root.path().join(".vesper-scope-id").exists());

    holder::reset_for_tests();
}

/// 2: the holder is first-resolution-wins; a mid-process change of
/// directory cannot re-scope the process (boot-only discipline).
#[test]
fn holder_is_first_resolution_wins() {
    let _guard = holder_lock();
    let first_root = tempfile::tempdir().expect("first");
    let second_root = tempfile::tempdir().expect("second");
    project_fixture(first_root.path(), "first");
    project_fixture(second_root.path(), "second");

    let first = holder::install_for_root(first_root.path())
        .expect("first resolves")
        .expect("installed");
    let first_id = first.id().to_string();

    // A second install attempt for a different directory observes the
    // first-resolved scope: the holder keeps the boot resolution.
    let still_first = holder::install_for_root(second_root.path())
        .expect("later install does not fail")
        .expect("installed");
    assert_eq!(
        still_first.id().to_string(),
        first_id,
        "later installs observe the first-resolved scope, not their own"
    );

    holder::reset_for_tests();
}

/// 3: `AGENT_VESPER_EXTRA_SCOPES` is completely dormant by default — no
/// env var ⇒ no extra layer ⇒ the advertised skills surface is exactly the
/// project + global layers.
#[test]
fn extra_scopes_completely_dormant_by_default() {
    let _guard = holder_lock();
    let root = tempfile::tempdir().expect("project");
    let global = tempfile::tempdir().expect("global");
    let extra = tempfile::tempdir().expect("extra");
    project_fixture(root.path(), "project-skill");
    project_fixture(extra.path(), "extra-skill");
    // The extra directory is NOT registered anywhere: no env var points at
    // it. Resolving the scope must not mount it.
    let scope =
        vesper_agent::vro::scope::WorkspaceScope::resolve(&vesper_agent::vro::scope::ScopeInputs {
            root: root.path().to_path_buf(),
            global_root: global.path().to_path_buf(),
            ..vesper_agent::vro::scope::ScopeInputs::default()
        })
        .expect("scope resolves");
    assert!(
        scope.extra_roots().is_empty(),
        "no AGENT_VESPER_EXTRA_SCOPES ⇒ no extra roots"
    );
    assert!(
        !scope
            .skills()
            .entries()
            .iter()
            .any(|skill| skill.slug == "extra-skill"),
        "unmounted extra directory must not leak into the skills surface"
    );
    assert!(
        scope
            .skills()
            .entries()
            .iter()
            .any(|skill| skill.slug == "project-skill"),
        "project layer 0 skills remain visible"
    );
}

/// 4: renaming the project directory does not re-key the scope: the stamp
/// file travels with the directory and the id is read back, not re-derived
/// (PRD §6.3 mitigation, adopted by this PR).
#[test]
fn rename_does_not_rekey_the_scope() {
    let _guard = holder_lock();
    let parent = tempfile::tempdir().expect("parent");
    let original = parent.path().join("original-name");
    std::fs::create_dir_all(original.join(".agent-vesper").join("skills")).expect("skills dir");

    let before = holder::install_for_root(&original)
        .expect("resolve before rename")
        .expect("installed");
    let before_id = before.id().to_string();

    // Rename the directory (same content, new path).
    let renamed = parent.path().join("renamed-project");
    std::fs::rename(&original, &renamed).expect("rename");

    holder::reset_for_tests();
    let after = holder::install_for_root(&renamed)
        .expect("resolve after rename")
        .expect("installed");
    assert_eq!(
        before_id,
        after.id().to_string(),
        "the .vesper-scope-id stamp survives the rename: stores stay keyed"
    );

    holder::reset_for_tests();
}

/// Reset clears the holder so a subsequent install resolves fresh state.
#[test]
fn reset_clears_the_holder() {
    let _guard = holder_lock();
    let root = tempfile::tempdir().expect("project");
    let first = holder::install_for_root(root.path())
        .expect("resolve")
        .expect("installed");
    assert!(holder::shared().is_some());
    holder::reset_for_tests();
    assert!(
        holder::shared().is_none(),
        "reset must clear the process-global holder"
    );
    let second = holder::install_for_root(root.path())
        .expect("resolve again")
        .expect("installed");
    assert_eq!(
        first.id(),
        second.id(),
        "re-resolving the same root yields the stamp-pinned id"
    );
    holder::reset_for_tests();
}
