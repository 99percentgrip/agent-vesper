//! Versioned, provider-neutral implementation acceptance data. No I/O.

use serde::{Deserialize, Serialize};

pub const ACCEPTANCE_VERSION: u32 = 1;
pub const ACCEPTANCE_COLLECTOR: &str = "vesper-native-cargo-v1";
pub const MAX_REQUIREMENTS: usize = 256;
pub const MAX_CHECKS: usize = 128;

/// A source paragraph is retained independently of model-generated requirements.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceSource {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceRequirement {
    pub id: String,
    pub description: String,
    pub source_ids: Vec<String>,
    pub scenarios: Vec<AcceptanceScenario>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceScenario {
    pub id: String,
    pub assertion: String,
    /// Explicit product scope, such as ACP, TUI, or a target platform.
    pub scope: String,
    pub evidence: AcceptanceEvidenceKind,
    pub platform: AcceptancePlatform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptancePlatform {
    Any,
    Linux,
    Macos,
    Windows,
}
impl AcceptancePlatform {
    pub fn matches_host(self) -> bool {
        match self {
            Self::Any => true,
            Self::Linux => cfg!(target_os = "linux"),
            Self::Macos => cfg!(target_os = "macos"),
            Self::Windows => cfg!(target_os = "windows"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceEvidenceKind {
    Unit,
    Integration,
    Native,
    Performance,
}

/// Non-normative source still needs an explicit classification and review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceContext {
    pub source_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceContract {
    pub version: u32,
    pub objective: String,
    pub requirements: Vec<AcceptanceRequirement>,
    pub context: Vec<AcceptanceContext>,
}

/// A check selects one exact Rust test. A model cannot supply shell syntax or
/// manufacture evidence by choosing an always-successful arbitrary command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCheck {
    pub id: String,
    pub scenario_ids: Vec<String>,
    pub evidence: AcceptanceEvidenceKind,
    pub platform: AcceptancePlatform,
    pub scope: String,
    pub package: String,
    /// Integration-test target; None selects the library test target.
    pub target: Option<String>,
    pub test: String,
    pub features: Vec<String>,
    pub all_features: bool,
    pub ignored: bool,
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceReview {
    /// Every source paragraph must be inspected in coverage review.
    pub inspected_source_ids: Vec<String>,
    /// Every scenario must be inspected in implementation review.
    pub inspected_scenario_ids: Vec<String>,
    pub findings: Vec<AcceptanceFinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceFinding {
    pub subject: String,
    pub evidence: String,
    pub repair: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceState {
    Missing,
    Failed,
    Stale,
    Inconclusive,
    Verified,
}

/// Export is an audit record, never authority to import trusted receipts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceReceipt {
    pub version: u32,
    pub collector: String,
    pub observed_at_unix_ms: u64,
    pub run_id: u64,
    pub check_id: String,
    pub contract_digest: String,
    pub checks_digest: String,
    pub source_digest: String,
    pub environment: String,
    pub argv: Vec<String>,
    pub test: String,
    pub state: AcceptanceState,
    pub elapsed_ms: u64,
    pub output_digest: String,
    pub diagnostic: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceGap {
    pub subject: String,
    pub state: AcceptanceState,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceReport {
    pub version: u32,
    pub objective: String,
    pub contract_digest: String,
    pub source_digest: String,
    pub verified_scenarios: usize,
    pub total_scenarios: usize,
    pub gaps: Vec<AcceptanceGap>,
    pub receipts: Vec<AcceptanceReceipt>,
}

impl AcceptanceReport {
    #[must_use]
    pub fn is_verified(&self) -> bool {
        self.version == ACCEPTANCE_VERSION
            && self.total_scenarios > 0
            && self.verified_scenarios == self.total_scenarios
            && self.gaps.is_empty()
    }

    /// The authoritative status is rendered from data, never model prose.
    #[must_use]
    pub fn render(&self) -> String {
        let status = if self.is_verified() {
            "VERIFIED"
        } else {
            "INCOMPLETE"
        };
        let mut text = format!(
            "Implementation acceptance: {status}\n{}\nVerified scenarios: {}/{}\n",
            self.objective, self.verified_scenarios, self.total_scenarios
        );
        for gap in &self.gaps {
            text.push_str(&format!(
                "- {}: {:?} — {}\n",
                gap.subject, gap.state, gap.reason
            ));
        }
        if self.is_verified() {
            text.push_str(&format!(
                "Source: {}\nContract: {}\nVerified against the recorded scope and checks; this is not a guarantee of zero undiscovered defects.\n",
                self.source_digest, self.contract_digest
            ));
        }
        text
    }
}
