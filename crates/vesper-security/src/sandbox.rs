use serde::{Deserialize, Serialize};

/// Honest capability discovery result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityStatus {
    /// Capability is available and verified by the backend.
    Available,
    /// Capability is unavailable.
    Unavailable,
    /// Capability has not been probed.
    Unknown,
}

/// Minimum requested process isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationRequirement {
    /// No OS isolation requested.
    None,
    /// Descendant ownership and cleanup.
    ProcessTree,
    /// Workspace-scoped filesystem writes.
    Filesystem,
    /// Network namespace/profile restriction.
    Network,
    /// Process, filesystem, and network isolation.
    Full,
}

/// Ordered security strength used for fail-closed requirement checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecurityStrength {
    /// No OS boundary.
    None,
    /// Process ownership only.
    Process,
    /// Process plus filesystem boundary.
    Filesystem,
    /// Process, filesystem, and network boundary.
    Full,
}

/// Describes a backend without claiming unavailable protection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxCapabilities {
    /// Backend display identifier.
    pub backend: String,
    /// Descendant cleanup.
    pub process_tree: CapabilityStatus,
    /// Filesystem write boundary.
    pub filesystem: CapabilityStatus,
    /// Network boundary.
    pub network: CapabilityStatus,
    /// Overall verified strength.
    pub strength: SecurityStrength,
}

impl SandboxCapabilities {
    /// Returns whether this backend satisfies the requested isolation.
    #[must_use]
    pub const fn satisfies(&self, requirement: IsolationRequirement) -> bool {
        match requirement {
            IsolationRequirement::None => true,
            IsolationRequirement::ProcessTree => {
                matches!(self.process_tree, CapabilityStatus::Available)
            }
            IsolationRequirement::Filesystem => {
                matches!(self.process_tree, CapabilityStatus::Available)
                    && matches!(self.filesystem, CapabilityStatus::Available)
            }
            IsolationRequirement::Network => {
                matches!(self.process_tree, CapabilityStatus::Available)
                    && matches!(self.network, CapabilityStatus::Available)
            }
            IsolationRequirement::Full => {
                matches!(self.process_tree, CapabilityStatus::Available)
                    && matches!(self.filesystem, CapabilityStatus::Available)
                    && matches!(self.network, CapabilityStatus::Available)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_only_backend_fails_closed_for_stronger_requirements() {
        let windows_job = SandboxCapabilities {
            backend: "windows-job".into(),
            process_tree: CapabilityStatus::Available,
            filesystem: CapabilityStatus::Unavailable,
            network: CapabilityStatus::Unavailable,
            strength: SecurityStrength::Process,
        };
        assert!(windows_job.satisfies(IsolationRequirement::ProcessTree));
        assert!(!windows_job.satisfies(IsolationRequirement::Filesystem));
        assert!(!windows_job.satisfies(IsolationRequirement::Full));
    }
}
