//! Deterministic acceptance policy and the hosted completion boundary.

use std::collections::BTreeSet;
use vesper_domain::acceptance::*;

use crate::executor::{ToolContext, ToolFuture};

/// Publication filter shared by parent and delegated execution paths. Tool and
/// progress events remain visible; model prose cannot announce parent success.
pub struct HoldCompletionProgress(pub std::sync::Arc<dyn crate::AgentProgressPort>);
impl crate::AgentProgressPort for HoldCompletionProgress {
    fn emit(&self, event: crate::AgentProgressEvent) {
        if !matches!(
            event,
            crate::AgentProgressEvent::ContentDelta { .. }
                | crate::AgentProgressEvent::ReasoningDelta { .. }
        ) {
            self.0.emit(event);
        }
    }
}

/// Hosts retain this service outside provider history and compaction. A stop is
/// a request for evaluation, never permission to mark the objective verified.
pub trait CompletionPort: Send + Sync {
    fn active(&self) -> bool;
    fn prepare<'a>(&'a self, _context: &'a ToolContext) -> ToolFuture<'a, Result<String, String>> {
        Box::pin(async { Ok(String::new()) })
    }
    fn evaluate<'a>(&'a self, context: &'a ToolContext) -> ToolFuture<'a, AcceptanceReport>;
    fn status(&self) -> AcceptanceReport;
}

#[must_use]
pub fn verification_command_allowed(context: &ToolContext, command: &str) -> bool {
    context.firewall.as_ref().is_none_or(|firewall| {
        !matches!(
            firewall.scan(command).decision,
            vesper_policy::firewall::RuleDecision::Deny
        )
    })
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_:.".contains(&c))
}

/// Reject omitted paragraphs and empty/duplicate/vague contract structure before
/// any model review. Semantic coverage still requires independent inspection.
pub fn validate_contract(
    contract: &AcceptanceContract,
    sources: &[AcceptanceSource],
) -> Result<(), String> {
    if contract.version != ACCEPTANCE_VERSION
        || contract.objective.trim().is_empty()
        || contract.objective.len() > 4096
        || contract.requirements.is_empty()
        || contract.requirements.len() > MAX_REQUIREMENTS
        || sources.is_empty()
        || sources.len() > MAX_REQUIREMENTS
        || contract.context.len() > MAX_REQUIREMENTS
    {
        return Err("invalid or unsupported acceptance contract bounds".into());
    }
    let source_ids: BTreeSet<_> = sources.iter().map(|s| s.id.as_str()).collect();
    if source_ids.len() != sources.len() {
        return Err("duplicate source ID".into());
    }
    let mut covered = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut scenarios = BTreeSet::new();
    for req in &contract.requirements {
        if !valid_id(&req.id)
            || !ids.insert(&req.id)
            || req.description.trim().is_empty()
            || req.description.len() > 4096
            || req.source_ids.is_empty()
            || req.source_ids.len() > MAX_REQUIREMENTS
            || req.scenarios.is_empty()
            || req.scenarios.len() > 32
        {
            return Err("invalid requirement identity, description or scenarios".into());
        }
        for source in &req.source_ids {
            if !source_ids.contains(source.as_str()) {
                return Err("unknown source reference".into());
            }
            covered.insert(source.as_str());
        }
        for scenario in &req.scenarios {
            if !valid_id(&scenario.id)
                || !scenarios.insert(&scenario.id)
                || scenario.assertion.trim().len() < 10
                || scenario.assertion.len() > 4096
                || scenario.scope.trim().is_empty()
                || scenario.scope.len() > 128
            {
                return Err("invalid or duplicate scenario".into());
            }
        }
    }
    if scenarios.len() > MAX_REQUIREMENTS * 4 {
        return Err("too many scenarios".into());
    }
    for context in &contract.context {
        if !source_ids.contains(context.source_id.as_str())
            || context.reason.trim().len() < 10
            || context.reason.len() > 1024
            || !covered.insert(context.source_id.as_str())
        {
            return Err("invalid or overlapping context classification".into());
        }
    }
    if covered != source_ids {
        return Err("unmapped PRD source paragraphs remain".into());
    }
    Ok(())
}

pub fn validate_checks(
    contract: &AcceptanceContract,
    checks: &[AcceptanceCheck],
) -> Result<(), String> {
    let scenario_count: usize = contract
        .requirements
        .iter()
        .map(|r| r.scenarios.len())
        .sum();
    let unique_scenarios: BTreeSet<_> = contract
        .requirements
        .iter()
        .flat_map(|r| &r.scenarios)
        .map(|s| &s.id)
        .collect();
    if contract.version != ACCEPTANCE_VERSION
        || contract.requirements.is_empty()
        || contract.requirements.len() > MAX_REQUIREMENTS
        || scenario_count == 0
        || scenario_count > MAX_REQUIREMENTS * 4
        || unique_scenarios.len() != scenario_count
    {
        return Err("invalid contract version or scenario identities".into());
    }
    if checks.is_empty() || checks.len() > MAX_CHECKS {
        return Err("checks must contain 1–128 exact tests".into());
    }
    let scenarios: std::collections::BTreeMap<_, _> = contract
        .requirements
        .iter()
        .flat_map(|r| &r.scenarios)
        .map(|s| (s.id.as_str(), s))
        .collect();
    let mut ids = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for check in checks {
        if !valid_id(&check.id)
            || !ids.insert(&check.id)
            || !valid_id(&check.package)
            || !valid_id(&check.test)
            || check.target.as_ref().is_some_and(|t| !valid_id(t))
            || check.scenario_ids.is_empty()
            || check.scenario_ids.len() > scenarios.len()
            || check.features.len() > 32
            || check.features.iter().any(|f| !valid_id(f))
            || check.timeout_seconds == 0
            || check.timeout_seconds > 1800
        {
            return Err("invalid check identity, exact test selector or bounds".into());
        }
        let mut unique = BTreeSet::new();
        for id in &check.scenario_ids {
            let scenario = scenarios
                .get(id.as_str())
                .ok_or("check references unknown scenario")?;
            if !unique.insert(id)
                || check.evidence != scenario.evidence
                || check.platform != scenario.platform
                || check.scope != scenario.scope
            {
                return Err("check evidence class or scope does not match scenario".into());
            }
            covered.insert(id.as_str());
        }
    }
    if covered.len() != scenarios.len() {
        return Err("required scenarios lack checks".into());
    }
    Ok(())
}

pub fn validate_review(
    contract: &AcceptanceContract,
    sources: &[AcceptanceSource],
    review: &AcceptanceReview,
) -> Result<(), String> {
    let expected_sources: BTreeSet<_> = sources.iter().map(|s| &s.id).collect();
    let expected_scenarios: BTreeSet<_> = contract
        .requirements
        .iter()
        .flat_map(|r| &r.scenarios)
        .map(|s| &s.id)
        .collect();
    let inspected_sources: BTreeSet<_> = review.inspected_source_ids.iter().collect();
    let inspected_scenarios: BTreeSet<_> = review.inspected_scenario_ids.iter().collect();
    if expected_sources != inspected_sources
        || expected_scenarios != inspected_scenarios
        || inspected_sources.len() != review.inspected_source_ids.len()
        || inspected_scenarios.len() != review.inspected_scenario_ids.len()
        || review.findings.len() > MAX_REQUIREMENTS
    {
        return Err("review did not inspect every source and scenario exactly once".into());
    }
    for f in &review.findings {
        if (!expected_sources.iter().any(|s| s.as_str() == f.subject)
            && !expected_scenarios.iter().any(|s| s.as_str() == f.subject))
            || f.evidence.trim().len() < 10
            || f.evidence.len() > 4096
            || f.repair.trim().len() < 10
            || f.repair.len() > 4096
        {
            return Err(
                "review finding needs a known subject, concrete evidence and repair".into(),
            );
        }
    }
    Ok(())
}

/// Evaluates receipts already authenticated by the collector. Deserializing a
/// report alone never creates trusted collector state.
#[allow(
    clippy::too_many_arguments,
    reason = "pure evaluator explicitly receives all independent evidence identities"
)]
pub fn evaluate_receipts(
    contract: &AcceptanceContract,
    checks: &[AcceptanceCheck],
    receipts: &[AcceptanceReceipt],
    contract_digest: &str,
    checks_digest: &str,
    source_digest: &str,
    environment: &str,
    findings: &[AcceptanceFinding],
) -> AcceptanceReport {
    let mut report = AcceptanceReport {
        version: ACCEPTANCE_VERSION,
        objective: contract.objective.clone(),
        contract_digest: contract_digest.into(),
        source_digest: source_digest.into(),
        verified_scenarios: 0,
        total_scenarios: contract
            .requirements
            .iter()
            .map(|r| r.scenarios.len())
            .sum(),
        gaps: Vec::new(),
        receipts: receipts.to_vec(),
    };
    if let Err(error) = validate_checks(contract, checks) {
        report.gaps.push(AcceptanceGap {
            subject: "checks".into(),
            state: AcceptanceState::Missing,
            reason: error,
        });
        return report;
    }
    let mut run_ids = BTreeSet::new();
    if receipts
        .iter()
        .any(|r| !run_ids.insert(r.run_id) || !checks.iter().any(|c| c.id == r.check_id))
    {
        report.gaps.push(AcceptanceGap {
            subject: "receipts".into(),
            state: AcceptanceState::Inconclusive,
            reason: "duplicate or unknown receipt identity".into(),
        });
        return report;
    }
    for scenario in contract.requirements.iter().flat_map(|r| &r.scenarios) {
        let mut scenario_gaps = Vec::new();
        for check in checks
            .iter()
            .filter(|c| c.scenario_ids.contains(&scenario.id))
        {
            let matching: Vec<_> = receipts.iter().filter(|r| r.check_id == check.id).collect();
            let (state, reason) = match matching.as_slice() {
                [] => (
                    AcceptanceState::Missing,
                    format!("run acceptance_verify for check {}", check.id),
                ),
                [receipt]
                    if receipt.version != ACCEPTANCE_VERSION
                        || receipt.collector != ACCEPTANCE_COLLECTOR
                        || receipt.observed_at_unix_ms == 0 =>
                {
                    (
                        AcceptanceState::Inconclusive,
                        "unsupported receipt version, collector or observation time".into(),
                    )
                }
                [receipt]
                    if receipt.contract_digest != contract_digest
                        || receipt.checks_digest != checks_digest
                        || receipt.source_digest != source_digest
                        || receipt.environment != environment =>
                {
                    (
                        AcceptanceState::Stale,
                        format!(
                            "{} evidence belongs to different source, contract, checks or environment",
                            check.id
                        ),
                    )
                }
                [receipt] if receipt.test != check.test || receipt.argv != cargo_argv(check) => (
                    AcceptanceState::Inconclusive,
                    "receipt does not match the admitted exact test".into(),
                ),
                [receipt] => (receipt.state, receipt.diagnostic.clone()),
                _ => (
                    AcceptanceState::Inconclusive,
                    "multiple receipts for one check".into(),
                ),
            };
            if state != AcceptanceState::Verified {
                scenario_gaps.push(AcceptanceGap {
                    subject: scenario.id.clone(),
                    state,
                    reason,
                });
            }
        }
        if scenario_gaps.is_empty() {
            report.verified_scenarios += 1;
        }
        report.gaps.extend(scenario_gaps);
    }
    report.gaps.extend(findings.iter().map(|f| AcceptanceGap {
        subject: f.subject.clone(),
        state: AcceptanceState::Failed,
        reason: format!("{}; repair: {}", f.evidence, f.repair),
    }));
    report
}

#[must_use]
pub fn cargo_argv(check: &AcceptanceCheck) -> Vec<String> {
    let mut argv = vec![
        "cargo".into(),
        "test".into(),
        "--offline".into(),
        "--locked".into(),
        "--color".into(),
        "never".into(),
        "-p".into(),
        check.package.clone(),
    ];
    match &check.target {
        Some(t) => argv.extend(["--test".into(), t.clone()]),
        None => argv.push("--lib".into()),
    }
    if check.all_features {
        argv.push("--all-features".into());
    }
    if !check.features.is_empty() {
        argv.extend(["--features".into(), check.features.join(",")]);
    }
    argv.extend([
        check.test.clone(),
        "--".into(),
        "--exact".into(),
        "--test-threads=1".into(),
    ]);
    if check.ignored {
        argv.push("--ignored".into());
    }
    argv
}
