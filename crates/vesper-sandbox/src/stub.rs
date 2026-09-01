//! Non-Linux honest stub (VRO-13 PR-3).
//!
//! There is no namespaces backend off Linux. This stub reports every
//! capability `Unavailable` so any isolation demand fails closed through
//! `SandboxCapabilities::satisfies` — exactly the fail-closed path
//! `vesper-security` already defines. It never fakes success and never
//! constructs a sandbox.

use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};

use crate::{
    Argv, ExecOutput, SandboxBackend, SandboxError, SandboxFuture, SandboxHandle, SandboxSpec,
};

/// Platform stub: unavailable everywhere, honestly.
#[derive(Debug, Default)]
pub struct UnavailableBackend;

fn caps() -> SandboxCapabilities {
    SandboxCapabilities {
        backend: "unavailable-stub".to_owned(),
        process_tree: CapabilityStatus::Unavailable,
        filesystem: CapabilityStatus::Unavailable,
        network: CapabilityStatus::Unavailable,
        strength: SecurityStrength::None,
    }
}

impl UnavailableBackend {
    fn denial(requirement: IsolationRequirement) -> SandboxError {
        SandboxError::CapabilityUnavailable {
            requirement,
            capabilities: caps(),
        }
    }
}

impl SandboxBackend for UnavailableBackend {
    fn capabilities(&self) -> SandboxCapabilities {
        caps()
    }

    fn provision<'a>(
        &'a self,
        _spec: &'a SandboxSpec,
    ) -> SandboxFuture<'a, Result<SandboxHandle, SandboxError>> {
        Box::pin(async { Err(Self::denial(IsolationRequirement::ProcessTree)) })
    }

    fn run<'a>(
        &'a self,
        _handle: &'a SandboxHandle,
        _argv: &'a Argv,
    ) -> SandboxFuture<'a, Result<ExecOutput, SandboxError>> {
        Box::pin(async { Err(Self::denial(IsolationRequirement::ProcessTree)) })
    }

    fn teardown<'a>(
        &'a self,
        _handle: SandboxHandle,
    ) -> SandboxFuture<'a, Result<(), SandboxError>> {
        // A handle can never exist on this platform; refuse honestly.
        Box::pin(async {
            Err(SandboxError::Teardown(
                "no sandbox backend on this platform".into(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_reports_everything_unavailable_and_fails_closed() {
        let stub = UnavailableBackend;
        let caps = stub.capabilities();
        assert_eq!(caps.process_tree, CapabilityStatus::Unavailable);
        assert_eq!(caps.filesystem, CapabilityStatus::Unavailable);
        assert_eq!(caps.network, CapabilityStatus::Unavailable);
        assert!(!caps.satisfies(IsolationRequirement::ProcessTree));
        assert!(!caps.satisfies(IsolationRequirement::Filesystem));
        assert!(!caps.satisfies(IsolationRequirement::Network));
        assert!(!caps.satisfies(IsolationRequirement::Full));
    }
}
