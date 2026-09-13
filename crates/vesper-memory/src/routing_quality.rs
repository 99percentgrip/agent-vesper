//! Bounded metadata retrieval and execution-contract comparison.
//!
//! This module never reads skill bodies, mutates the library, or grants tool
//! authority. The shared orchestrator remains the policy and loading boundary.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::SkillMetadata;

pub const MAX_ROUTING_DESCRIPTOR_BYTES: usize = 4_096;
pub const MAX_ROUTING_CANDIDATES: usize = 12;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    #[default]
    Standard,
    Enhanced,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingEffect {
    #[default]
    ReadOnly,
    Workspace,
    External,
}

/// Authored data describing the task contract, never executable instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingDescriptor {
    pub version: u8,
    /// Revision supplied by the owning catalog snapshot, not a model assertion.
    pub revision: String,
    pub family: String,
    pub purpose: String,
    #[serde(default)]
    pub use_when: Vec<String>,
    #[serde(default)]
    pub avoid_when: Vec<String>,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub preconditions: Vec<String>,
    pub effects: RoutingEffect,
    #[serde(default)]
    pub positive_examples: Vec<String>,
    #[serde(default)]
    pub negative_examples: Vec<String>,
}

impl RoutingDescriptor {
    pub fn parse(text: &str, revision: &str) -> Result<Self, &'static str> {
        if text.len() > MAX_ROUTING_DESCRIPTOR_BYTES {
            return Err("descriptor exceeds byte limit");
        }
        let descriptor: Self = serde_json::from_str(text).map_err(|_| "invalid descriptor")?;
        descriptor.validate(revision)?;
        Ok(descriptor)
    }

    pub fn validate(&self, revision: &str) -> Result<(), &'static str> {
        if self.version != 1 {
            return Err("unsupported descriptor version");
        }
        if self.revision != revision {
            return Err("stale descriptor revision");
        }
        for field in [&self.revision, &self.family, &self.purpose] {
            if field.trim().is_empty() || field.chars().count() > 240 {
                return Err("invalid descriptor field");
            }
        }
        for list in [
            &self.use_when,
            &self.avoid_when,
            &self.inputs,
            &self.outputs,
            &self.preconditions,
            &self.positive_examples,
            &self.negative_examples,
        ] {
            if list.len() > 8
                || list
                    .iter()
                    .any(|value| value.trim().is_empty() || value.chars().count() > 240)
            {
                return Err("descriptor list exceeds bounds");
            }
        }
        if self.positive_examples.len() > 3 || self.negative_examples.len() > 3 {
            return Err("descriptor example limit exceeded");
        }
        if serde_json::to_vec(self)
            .map_err(|_| "invalid descriptor")?
            .len()
            > MAX_ROUTING_DESCRIPTOR_BYTES
        {
            return Err("descriptor exceeds byte limit");
        }
        Ok(())
    }
}

/// These hints narrow relevance. They never authorize an operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutingTask {
    pub artifact: Option<String>,
    pub maximum_effect: Option<RoutingEffect>,
    /// None means resource availability is unknown, not that all resources exist.
    pub available_resources: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingRejection {
    ArtifactMismatch,
    EffectMismatch,
    MissingResource,
    NegativeExample,
}

#[derive(Debug, Clone)]
struct Document {
    slug: String,
    frequencies: BTreeMap<String, usize>,
    length: usize,
    descriptor: Option<RoutingDescriptor>,
}

/// Snapshot identity is provided by the store, never by the descriptor itself.
#[derive(Debug, Clone)]
pub struct RoutingCatalogEntry {
    pub metadata: SkillMetadata,
    pub revision: String,
    pub descriptor: Option<RoutingDescriptor>,
}

/// Index of already validated metadata. No filesystem or network authority.
#[derive(Debug, Clone, Default)]
pub struct RoutingIndex {
    documents: Vec<Document>,
    document_frequency: BTreeMap<String, usize>,
    average_length: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoutingMatch {
    pub slug: String,
    /// Relevance score, never a calibrated confidence probability.
    pub score: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoutingSearch {
    pub candidates: Vec<RoutingMatch>,
    pub rejected: Vec<(String, RoutingRejection)>,
}

impl RoutingIndex {
    /// Input is the policy-eligible catalog. Duplicate IDs and unbounded input
    /// are errors rather than silent truncation or order-dependent shadowing.
    pub fn build(entries: &[RoutingCatalogEntry]) -> Result<Self, &'static str> {
        if entries.len() > crate::MAX_SKILL_FILES {
            return Err("routing catalog exceeds limit");
        }
        let mut index = Self::default();
        let mut ids = BTreeSet::new();
        for entry in entries {
            let metadata = &entry.metadata;
            let descriptor = &entry.descriptor;
            if !ids.insert(metadata.slug.clone()) {
                return Err("duplicate routing identity");
            }
            if let Some(descriptor) = descriptor {
                descriptor.validate(&entry.revision)?;
                let required_effect = match metadata.risk {
                    crate::SkillRisk::ReadOnly => RoutingEffect::ReadOnly,
                    crate::SkillRisk::Mutating => RoutingEffect::Workspace,
                    crate::SkillRisk::External => RoutingEffect::External,
                };
                if descriptor.effects < required_effect {
                    return Err("descriptor contradicts authoritative effects");
                }
            }
            let mut text = format!(
                "{} {} {} {} {}",
                metadata.name,
                metadata.description,
                metadata.tags.join(" "),
                metadata.triggers.join(" "),
                metadata.file_extensions.join(" ")
            );
            if let Some(descriptor) = descriptor {
                text.push_str(&format!(
                    " {} {} {} {} {} {}",
                    descriptor.purpose,
                    descriptor.use_when.join(" "),
                    descriptor.inputs.join(" "),
                    descriptor.outputs.join(" "),
                    descriptor.preconditions.join(" "),
                    descriptor.positive_examples.join(" ")
                ));
            }
            let mut frequencies = BTreeMap::new();
            for token in tokens(&text) {
                *frequencies.entry(token).or_insert(0) += 1;
            }
            for token in frequencies.keys() {
                *index.document_frequency.entry(token.clone()).or_insert(0) += 1;
            }
            index.documents.push(Document {
                slug: metadata.slug.clone(),
                length: frequencies.values().sum(),
                frequencies,
                descriptor: descriptor.clone(),
            });
        }
        index.documents.sort_by(|a, b| a.slug.cmp(&b.slug));
        index.average_length = if index.documents.is_empty() {
            1.0
        } else {
            index.documents.iter().map(|doc| doc.length).sum::<usize>() as f64
                / index.documents.len() as f64
        }
        .max(1.0);
        Ok(index)
    }

    pub fn search(&self, prompt: &str, task: &RoutingTask) -> RoutingSearch {
        let query: BTreeSet<_> = tokens(prompt).into_iter().collect();
        let mut result = RoutingSearch::default();
        for document in &self.documents {
            if let Some(descriptor) = &document.descriptor
                && let Some(reason) = contract_rejection(descriptor, task, &query)
            {
                result.rejected.push((document.slug.clone(), reason));
                continue;
            }
            let mut score = 0.0;
            for term in &query {
                let tf = *document.frequencies.get(term).unwrap_or(&0) as f64;
                if tf == 0.0 {
                    continue;
                }
                let df = *self.document_frequency.get(term).unwrap_or(&0) as f64;
                let idf = (1.0 + (self.documents.len() as f64 - df + 0.5) / (df + 0.5)).ln();
                score += idf * (tf * 2.2)
                    / (tf + 1.2 * (0.25 + 0.75 * document.length as f64 / self.average_length));
            }
            if score > 0.0 {
                result.candidates.push(RoutingMatch {
                    slug: document.slug.clone(),
                    score,
                });
            }
        }
        result.candidates.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.slug.cmp(&b.slug))
        });
        result.candidates.truncate(MAX_ROUTING_CANDIDATES);
        result
    }
}

fn contract_rejection(
    descriptor: &RoutingDescriptor,
    task: &RoutingTask,
    query: &BTreeSet<String>,
) -> Option<RoutingRejection> {
    if task
        .maximum_effect
        .is_some_and(|maximum| descriptor.effects > maximum)
    {
        return Some(RoutingRejection::EffectMismatch);
    }
    if let Some(artifact) = &task.artifact
        && !descriptor.outputs.is_empty()
        && !descriptor
            .outputs
            .iter()
            .any(|output| output.eq_ignore_ascii_case(artifact))
    {
        return Some(RoutingRejection::ArtifactMismatch);
    }
    if let Some(resources) = &task.available_resources
        && descriptor
            .preconditions
            .iter()
            .any(|needed| !resources.contains(needed))
    {
        return Some(RoutingRejection::MissingResource);
    }
    if descriptor
        .avoid_when
        .iter()
        .chain(&descriptor.negative_examples)
        .any(|negative| {
            let terms: BTreeSet<_> = tokens(negative).into_iter().collect();
            !terms.is_empty() && terms.is_subset(query)
        })
    {
        return Some(RoutingRejection::NegativeExample);
    }
    None
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter_map(|word| {
            let lower = word.to_lowercase();
            if lower.len() < 2
                || [
                    "the", "and", "for", "with", "this", "that", "from", "into", "please", "can",
                    "you", "to", "of", "in", "it", "is", "an", "a",
                ]
                .contains(&lower.as_str())
            {
                None
            } else {
                Some(lower)
            }
        })
        .collect()
}
