//! A cached maintenance binary must validate the invoking worktree and its fixtures.
use std::{fs, path::Path, process::Command};

fn workspace(root: &Path) {
    fs::create_dir_all(root.join("xtask/src")).unwrap();
    fs::create_dir_all(root.join("fixtures")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"xtask\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("xtask/Cargo.toml"),
        "[package]\nname=\"xtask\"\nversion=\"0.0.0\"\n",
    )
    .unwrap();
    fs::write(root.join("xtask/src/main.rs"), "fn main() {}\n").unwrap();
}

#[test]
fn cached_binary_uses_each_invoking_workspace_and_fixture_corpus() {
    let temporary = tempfile::tempdir().unwrap();
    for name in ["first", "second"] {
        let root = temporary.path().join(name);
        workspace(&root);
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["fixtures", "validate"])
            .current_dir(root.join("xtask/src"))
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "an empty invocation corpus cannot pass"
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        let root = root.canonicalize().unwrap();
        assert!(stdout.contains(&root.display().to_string()), "{stdout}");
        assert!(
            stderr.contains(&root.join("fixtures").display().to_string()),
            "{stderr}"
        );
    }
}

#[test]
fn cached_binary_outside_workspace_refuses_build_directory_fallback() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["fixtures", "validate"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("refusing cached build-path fallback")
    );
}
