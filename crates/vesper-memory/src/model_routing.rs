//! Bounded two-phase model selection. No provider, network or tool authority.
use crate::routing_quality::{RoutingCatalogEntry, RoutingOptions, RoutingSearch};
use crate::{SkillExecutionMode, SkillInvocationPolicy, SkillRisk, SkillRoutingQuery, SkillSlug};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_SELECTION_METADATA_BYTES: usize = 8192;
pub const MAX_SELECTION_TASK_BYTES: usize = 8192;
pub const MAX_SELECTION_RESPONSE_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillOffer {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub actions: Vec<String>,
    pub outputs: Vec<String>,
    pub preconditions: Vec<String>,
    pub use_when: Vec<String>,
    pub avoid_when: Vec<String>,
    pub tools: Vec<String>,
    pub effect: String,
    pub execution: String,
}

/// Private snapshot fields cannot be supplied by model JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSkillSelection {
    offers: Vec<SkillOffer>,
    store_identity: Vec<u8>,
    catalog: Vec<RoutingCatalogEntry>,
    request_digest: Vec<u8>,
    options: RoutingOptions,
    tools: BTreeSet<String>,
    platform: String,
}
impl PreparedSkillSelection {
    pub(crate) fn new(
        store_identity: Vec<u8>,
        catalog: &[RoutingCatalogEntry],
        search: &RoutingSearch,
        query: &SkillRoutingQuery<'_>,
        options: &RoutingOptions,
    ) -> Result<Self, &'static str> {
        if query.prompt.len() > MAX_SELECTION_TASK_BYTES {
            return Err("model selection task exceeds byte budget");
        }
        let offers: Vec<_> = search
            .candidates
            .iter()
            .filter_map(|candidate| {
                let entry = catalog
                    .iter()
                    .find(|entry| entry.metadata.slug == candidate.slug)?;
                let metadata = &entry.metadata;
                // Eligibility has already excluded UserOnly. Defense in depth for a model offer.
                if metadata.invocation == SkillInvocationPolicy::UserOnly {
                    return None;
                }
                let descriptor = entry.descriptor.as_ref();
                Some(SkillOffer {
                    id: metadata.slug.clone(),
                    name: metadata.name.clone(),
                    purpose: descriptor
                        .map_or_else(|| metadata.description.clone(), |d| d.purpose.clone()),
                    actions: descriptor.map_or_else(Vec::new, |d| d.actions.clone()),
                    outputs: descriptor
                        .map_or_else(|| metadata.file_extensions.to_vec(), |d| d.outputs.clone()),
                    preconditions: descriptor.map_or_else(Vec::new, |d| d.preconditions.clone()),
                    use_when: descriptor.map_or_else(Vec::new, |d| d.use_when.clone()),
                    avoid_when: descriptor.map_or_else(Vec::new, |d| d.avoid_when.clone()),
                    tools: metadata.required_tools.to_vec(),
                    effect: match descriptor
                        .map(|d| d.effects)
                        .unwrap_or(match metadata.risk {
                            SkillRisk::ReadOnly => crate::routing_quality::RoutingEffect::ReadOnly,
                            SkillRisk::Mutating => crate::routing_quality::RoutingEffect::Workspace,
                            SkillRisk::External => crate::routing_quality::RoutingEffect::External,
                        }) {
                        crate::routing_quality::RoutingEffect::ReadOnly => "read_only",
                        crate::routing_quality::RoutingEffect::Workspace => "workspace",
                        crate::routing_quality::RoutingEffect::External => "external",
                    }
                    .into(),
                    execution: match metadata.execution {
                        SkillExecutionMode::Inline => "inline",
                        SkillExecutionMode::Isolated => "isolated",
                    }
                    .into(),
                })
            })
            .collect();
        if offers.len() > crate::routing_quality::MAX_ROUTING_CANDIDATES {
            return Err("model selection shortlist exceeds bound");
        }
        let encoded =
            serde_json::to_vec(&offers).map_err(|_| "cannot encode selection metadata")?;
        if encoded.len() > MAX_SELECTION_METADATA_BYTES {
            return Err("model selection metadata exceeds byte budget");
        }
        let mut catalog = catalog.to_vec();
        catalog.sort_by(|a, b| a.metadata.slug.cmp(&b.metadata.slug));
        Ok(Self {
            offers,
            store_identity,
            catalog,
            request_digest: Sha256::digest(query.prompt.as_bytes()).to_vec(),
            options: options.clone(),
            tools: query.available_tools.clone(),
            platform: query.platform.into(),
        })
    }
    pub fn offers(&self) -> &[SkillOffer] {
        &self.offers
    }
    pub fn request(&self, task: &str) -> Result<String, &'static str> {
        if task.len() > MAX_SELECTION_TASK_BYTES
            || Sha256::digest(task.as_bytes()).as_slice() != self.request_digest
        {
            return Err("selection task does not match prepared request");
        }
        serde_json::to_string(&serde_json::json!({"task":task,"candidates":self.offers}))
            .map_err(|_| "cannot encode model selection request")
    }
    pub(crate) fn matches(
        &self,
        store_identity: &[u8],
        catalog: &[RoutingCatalogEntry],
        query: &SkillRoutingQuery<'_>,
        options: &RoutingOptions,
    ) -> bool {
        let mut catalog = catalog.to_vec();
        catalog.sort_by(|a, b| a.metadata.slug.cmp(&b.metadata.slug));
        self.store_identity == store_identity
            && self.catalog == catalog
            && self.options == *options
            && self.tools == *query.available_tools
            && self.platform == query.platform
            && Sha256::digest(query.prompt.as_bytes()).as_slice() == self.request_digest
            && query.explicit_skill.is_none()
    }
    pub fn parse_decision(&self, text: &str) -> Result<ModelSkillDecision, &'static str> {
        if text.len() > MAX_SELECTION_RESPONSE_BYTES {
            return Err("model selection response exceeds byte budget");
        }
        let decision: ModelSkillDecision =
            serde_json::from_str(text).map_err(|_| "invalid model selection response")?;
        self.validate(&decision)?;
        Ok(decision)
    }
    pub(crate) fn validate(&self, decision: &ModelSkillDecision) -> Result<(), &'static str> {
        if decision.skills.len() > crate::MAX_SELECTED_SKILLS {
            return Err("model selection exceeds skill limit");
        }
        if (decision.outcome == ModelSelectionOutcome::Selected) == decision.skills.is_empty() {
            return Err("model selection outcome contradicts IDs");
        }
        let mut unique = BTreeSet::new();
        for id in &decision.skills {
            if SkillSlug::new(id).is_err()
                || !unique.insert(id)
                || !self.offers.iter().any(|offer| offer.id == *id)
            {
                return Err("model selection identity is outside prepared shortlist");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSelectionOutcome {
    Selected,
    NoSkillNeeded,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSkillDecision {
    pub outcome: ModelSelectionOutcome,
    pub skills: Vec<String>,
}
