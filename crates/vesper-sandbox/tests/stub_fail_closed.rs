//! VRO-13 PR-3: the honest stub must fail closed **everywhere**, not only on
//! Linux. This test runs on all platforms so the non-Linux default backend
//! (`UnavailableBackend`) is continuously verified even though the rest of
//! the sandbox integration tests are Linux-only.

use vesper_security::IsolationRequirement;

#[test]
fn unavailable_stub_fails_closed_on_every_platform() {
    // On Linux with a fully-provisioned backend this exercises the real
    // backend; on every other host (and on restricted hosts like this
    // container) it exercises the stub — either way the contract is the
    // same: never fake success.
    let backend = vesper_sandbox::default_backend();
    let caps = backend.capabilities();
    if !caps.satisfies(IsolationRequirement::Filesystem) {
        // Fail-closed path: provision must refuse.
        let spec = vesper_sandbox::SandboxSpec::new(std::path::PathBuf::from("/tmp/ws"));
        let outcome = tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(backend.provision(&spec));
        match outcome {
            Ok(_) => panic!(
                "backend {:?} reported Filesystem unavailable but provision succeeded",
                caps.backend
            ),
            Err(vesper_sandbox::SandboxError::CapabilityUnavailable { .. }) => {}
            Err(other) => panic!("expected CapabilityUnavailable, got: {other}"),
        }
    }
    // Off-Linux the default backend IS the stub; verify its report too.
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(caps.backend, "unavailable-stub");
        assert!(!caps.satisfies(IsolationRequirement::Full));
        assert!(!caps.satisfies(IsolationRequirement::Network));
    }
}
