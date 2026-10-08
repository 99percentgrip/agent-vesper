//! Explicit hosted-platform acceptance; default foundation tests never access
//! an OS credential manager. Every native identity belongs to this fixture.

use std::{path::PathBuf, process::Command};
use vesper_auth::{CredentialId, SecureCredentialStore, StorageBackend};

const ID: CredentialId = CredentialId::new("fixture", "subscription");
const NEIGHBOR: CredentialId = CredentialId::new("neighbor", "api-key");

fn require_hosted_fixture() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("VESPER_NATIVE_CREDENTIAL_ACCEPTANCE").as_deref(),
        Ok("isolated-hosted-fixture")
    );
    assert!(matches!(
        std::env::var("RUNNER_OS").as_deref(),
        Ok("Windows" | "macOS" | "Linux")
    ));
}

fn synthetic_record(phase: &str) -> String {
    format!(
        "{{\"mode\":\"chatgpt\",\"tokens\":{{\"access_token\":\"{}\",\"refresh_token\":\"{}\",\"id_token\":\"{}\"}}}}",
        format!("synthetic-{phase}-access").repeat(400),
        format!("synthetic-{phase}-refresh").repeat(100),
        format!("synthetic-{phase}-identity").repeat(400),
    )
}

struct Cleanup(SecureCredentialStore);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.0.remove(ID);
        let _ = self.0.remove(NEIGHBOR);
    }
}

#[test]
#[ignore = "explicit hosted OS credential-manager acceptance only"]
fn hosted_native_credential_persistence() {
    require_hosted_fixture();
    let temporary = tempfile::tempdir().unwrap();
    let service = format!("vesper-ci-credential-{}", uuid::Uuid::new_v4().simple());
    // The native API takes a static service identity; this process has exactly
    // one bounded fixture, and its entries are removed before process exit.
    let service: &'static str = Box::leak(service.into_boxed_str());
    let vault = temporary.path().join("auth").join("fixture.json");
    let store = SecureCredentialStore::new(service, vault.clone());
    let cleanup = Cleanup(store.clone());
    assert!(store.load(ID).unwrap().is_none());
    store.store(NEIGHBOR, "synthetic-neighbor-key").unwrap();
    // Existing small native API keys remain readable before and after a large
    // record rotation through the same public production store.
    store.store(ID, "synthetic-legacy-key").unwrap();
    for phase in ["first", "rotated"] {
        let record = synthetic_record(phase);
        assert!(record.len() > 16 * 1024);
        let receipt = store.store(ID, &record).unwrap();
        if cfg!(windows) {
            assert_eq!(receipt.backend, StorageBackend::NativeKeyring);
            assert!(!vault.exists(), "Windows must not write a plaintext vault");
        }
        assert_eq!(store.load(ID).unwrap().unwrap().expose().as_str(), record);
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "hosted_native_credential_reload_child",
                "--show-output",
            ])
            .env("VESPER_CREDENTIAL_FIXTURE_SERVICE", service)
            .env("VESPER_CREDENTIAL_FIXTURE_VAULT", &vault)
            .env("VESPER_CREDENTIAL_FIXTURE_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            valid_reload_receipt(&child.stdout, child.status.success()),
            "fresh-process reload lacks one passing exact case"
        );
        println!(
            "native credential round-trip/restart {phase}: {:?}",
            receipt.backend
        );
    }
    store.remove(ID).unwrap();
    assert!(store.load(ID).unwrap().is_none());
    assert_eq!(
        store.load(NEIGHBOR).unwrap().unwrap().expose().as_str(),
        "synthetic-neighbor-key"
    );
    store.remove(NEIGHBOR).unwrap();
    drop(cleanup);
    println!("native credential sign-out and provider isolation verified");
}

#[test]
#[ignore = "spawned only by the explicit hosted native persistence fixture"]
fn hosted_native_credential_reload_child() {
    require_hosted_fixture();
    let service = std::env::var("VESPER_CREDENTIAL_FIXTURE_SERVICE").unwrap();
    assert!(service.starts_with("vesper-ci-credential-"));
    let suffix = service.strip_prefix("vesper-ci-credential-").unwrap();
    assert_eq!(suffix.len(), 32);
    assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let vault = PathBuf::from(std::env::var_os("VESPER_CREDENTIAL_FIXTURE_VAULT").unwrap());
    let phase = std::env::var("VESPER_CREDENTIAL_FIXTURE_PHASE").unwrap();
    assert!(matches!(phase.as_str(), "first" | "rotated"));
    let store = SecureCredentialStore::new(Box::leak(service.into_boxed_str()), vault);
    assert_eq!(
        store.load(ID).unwrap().unwrap().expose().as_str(),
        synthetic_record(&phase)
    );
}

fn valid_reload_receipt(output: &[u8], success: bool) -> bool {
    let Ok(output) = std::str::from_utf8(output) else {
        return false;
    };
    success
        && output
            .lines()
            .any(|line| line.trim() == "test hosted_native_credential_reload_child ... ok")
        && output
            .lines()
            .any(|line| line.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored;"))
}

#[test]
fn reload_receipt_rejects_missing_ignored_failed_and_zero_cases() {
    let passed = b"test hosted_native_credential_reload_child ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 1 filtered out;\n";
    assert!(valid_reload_receipt(passed, true));
    assert!(!valid_reload_receipt(passed, false));
    for output in [
        "test result: ok. 0 passed; 0 failed; 0 ignored; 2 filtered out;\n",
        "test hosted_native_credential_reload_child ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored;\n",
        "test renamed ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n",
    ] {
        assert!(!valid_reload_receipt(output.as_bytes(), true));
    }
}
