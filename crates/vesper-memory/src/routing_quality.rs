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
    pub actions: Vec<String>,
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
            &self.actions,
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
    pub action: Option<String>,
    pub artifact: Option<String>,
    pub maximum_effect: Option<RoutingEffect>,
    /// None means resource availability is unknown, not that all resources exist.
    pub available_resources: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingRejection {
    ArtifactMismatch,
    ActionMismatch,
    EffectMismatch,
    MissingResource,
    NegativeExample,
}

#[derive(Debug, Clone)]
struct Document {
    slug: String,
    frequencies: BTreeMap<String, usize>,
    length: usize,
    anchors: BTreeSet<String>,
    descriptor: Option<RoutingDescriptor>,
    effect: RoutingEffect,
}

/// Snapshot identity is provided by the store, never by the descriptor itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingCatalogEntry {
    pub metadata: SkillMetadata,
    pub revision: String,
    pub descriptor: Option<RoutingDescriptor>,
}

impl RoutingCatalogEntry {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Some(descriptor) = &self.descriptor {
            descriptor.validate(&self.revision)?;
            if descriptor.effects < self.required_effect() {
                return Err("descriptor contradicts authoritative effects");
            }
        }
        Ok(())
    }

    fn required_effect(&self) -> RoutingEffect {
        match self.metadata.risk {
            crate::SkillRisk::ReadOnly => RoutingEffect::ReadOnly,
            crate::SkillRisk::Mutating => RoutingEffect::Workspace,
            crate::SkillRisk::External => RoutingEffect::External,
        }
    }
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
    pub matched_terms: usize,
    pub query_terms: usize,
    /// Matches in identity/tags/declared artifacts, excluding generic actions.
    pub anchor_terms: usize,
    pub task_request: bool,
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
        let mut text_bytes = 0usize;
        let mut term_count = 0usize;
        for entry in entries {
            let metadata = &entry.metadata;
            let descriptor = &entry.descriptor;
            if !ids.insert(metadata.slug.clone()) {
                return Err("duplicate routing identity");
            }
            entry.validate()?;
            let required_effect = entry.required_effect();
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
            text_bytes = text_bytes.saturating_add(text.len());
            if text_bytes > 1_048_576 {
                return Err("routing metadata exceeds index byte budget");
            }
            let mut frequencies = BTreeMap::new();
            for token in tokens(&text) {
                *frequencies.entry(token).or_insert(0) += 1;
            }
            term_count += frequencies.len();
            if term_count > 64_000 {
                return Err("routing metadata exceeds index term budget");
            }
            for token in frequencies.keys() {
                *index.document_frequency.entry(token.clone()).or_insert(0) += 1;
            }
            let anchors = tokens(&format!(
                "{} {} {} {}",
                metadata.name,
                metadata.description,
                metadata.tags.join(" "),
                metadata.file_extensions.join(" ")
            ))
            .into_iter()
            .filter(|t| !generic_term(t))
            .collect();
            index.documents.push(Document {
                slug: metadata.slug.clone(),
                length: frequencies.values().sum(),
                anchors,
                frequencies,
                descriptor: descriptor.clone(),
                effect: descriptor.as_ref().map_or(required_effect, |d| d.effects),
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
        self.search_condition(prompt, task, true)
    }

    pub(crate) fn search_condition(
        &self,
        prompt: &str,
        task: &RoutingTask,
        contracts: bool,
    ) -> RoutingSearch {
        let query: BTreeSet<_> = tokens(prompt).into_iter().collect();
        let mut result = RoutingSearch::default();
        let task_request = task_request(prompt);
        for document in &self.documents {
            if contracts
                && task
                    .maximum_effect
                    .is_some_and(|maximum| document.effect > maximum)
            {
                result
                    .rejected
                    .push((document.slug.clone(), RoutingRejection::EffectMismatch));
                continue;
            }
            if contracts
                && let Some(descriptor) = &document.descriptor
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
                let field_weight = if document.anchors.contains(term) {
                    2.0
                } else {
                    1.0
                };
                score += field_weight * idf * (tf * 2.2)
                    / (tf + 1.2 * (0.25 + 0.75 * document.length as f64 / self.average_length));
            }
            if score > 0.0 {
                result.candidates.push(RoutingMatch {
                    slug: document.slug.clone(),
                    score,
                    matched_terms: query
                        .iter()
                        .filter(|term| document.frequencies.contains_key(*term))
                        .count(),
                    query_terms: query.len(),
                    anchor_terms: query.intersection(&document.anchors).count(),
                    task_request,
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
    if let Some(action) = &task.action
        && !descriptor.actions.is_empty()
        && !descriptor
            .actions
            .iter()
            .any(|a| a.eq_ignore_ascii_case(action))
    {
        return Some(RoutingRejection::ActionMismatch);
    }
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

fn generic_term(term: &str) -> bool {
    [
        "creat", "read", "edit", "write", "make", "use", "work", "file", "data", "tool", "task",
        "help", "do", "not", "anyth", "yet", "build", "find", "produc", "prepar", "updat",
        "provid", "perform", "execut", "without",
    ]
    .contains(&term)
}

// Language features only: no skill IDs, permissions, resource claims or body text.
fn task_request(text: &str) -> bool {
    let words: Vec<_> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(8)
        .map(str::to_lowercase)
        .collect();
    let first = match words.as_slice() {
        [a, b, c, ..] if ["can", "could", "would", "will"].contains(&a.as_str()) && b == "you" => {
            c.as_str()
        }
        [a, b, ..] if a == "please" => b.as_str(),
        [a, ..] => a.as_str(),
        _ => "",
    };
    [
        "create",
        "read",
        "write",
        "edit",
        "make",
        "build",
        "put",
        "turn",
        "combine",
        "merge",
        "clean",
        "update",
        "produce",
        "convert",
        "fill",
        "secure",
        "password",
        "find",
        "search",
        "determine",
        "review",
        "check",
        "outline",
        "prepare",
        "explore",
        "test",
        "exercise",
        "draw",
        "debug",
        "inspect",
        "use",
        "rewrite",
        "extract",
        "list",
        "summarize",
        "analyze",
        "execute",
        "publish",
        "send",
        "overwrite",
    ]
    .contains(&first)
        || words.starts_with(&["i".into(), "need".into()])
        || words.starts_with(&["help".into(), "me".into()])
}

fn tokens(text: &str) -> Vec<String> {
    let mut normalized = text.to_lowercase();
    for (phrase, canonical) in [
        ("portable document format", "pdf"),
        ("points of interest", "poi"),
        ("point of interest", "poi"),
        ("time zones", "timezones"),
        ("time zone", "timezone"),
    ] {
        normalized = normalized.replace(phrase, canonical);
    }
    let words: BTreeSet<_> = normalized.split(|c: char| !c.is_alphanumeric()).collect();
    if words.iter().any(|w| ["natural", "human"].contains(w))
        && words
            .iter()
            .any(|w| ["wording", "text", "voice", "prose", "paragraph", "writing"].contains(w))
    {
        normalized.push_str(" humanize");
    }
    let stemmer = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::English);
    normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(|word| {
            let lower = word.to_lowercase();
            if lower.len() < 2
                || [
                    "the", "and", "for", "with", "this", "that", "from", "into", "please", "can",
                    "you", "to", "of", "in", "it", "is", "an", "a", "do", "does", "did", "not",
                    "anything", "yet", "what", "how", "why", "these", "those", "them", "its",
                    "our", "your", "my", "me", "we", "us", "let", "am", "are", "be", "been",
                    "still", "here", "there", "now",
                ]
                .contains(&lower.as_str())
            {
                None
            } else {
                Some(match stemmer.stem(&lower).as_ref() {
                    "workbook" => "spreadsheet".into(),
                    "debugg" => "debug".into(),
                    "written" => "write".into(),
                    "outline" | "breakdown" => "plan".into(),
                    term => term.to_owned(),
                })
            }
        })
        .collect()
}

/// A project choice, separate from the skill library. Default is the baseline.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingPreferences {
    #[serde(default)]
    pub mode: RoutingMode,
    #[serde(default)]
    pub disabled: BTreeSet<String>,
}
impl RoutingPreferences {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.disabled.len() > crate::MAX_SKILL_FILES {
            return Err("too many disabled skills");
        }
        for slug in &self.disabled {
            crate::SkillSlug::new(slug).map_err(|_| "invalid disabled skill identity")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RoutingOutcome {
    Selected,
    #[default]
    NoSkillNeeded,
    Ambiguous,
    MissingPrecondition,
    ExplicitInvalid,
    Fallback,
}

/// No prompts or bodies. The host can render this without exposing task data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutingTrace {
    pub outcome: RoutingOutcome,
    pub mode: RoutingMode,
    pub reason: String,
}

/// Caller-owned context; resource assertions must come from validated host state.
#[derive(Debug, Clone, Default)]
pub struct RoutingOptions {
    pub preferences: RoutingPreferences,
    pub task: RoutingTask,
}

impl RoutingTask {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .action
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.chars().count() > 240)
            || self
                .artifact
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.chars().count() > 240)
            || self.available_resources.as_ref().is_some_and(|r| {
                r.len() > 8 || r.iter().any(|s| s.is_empty() || s.chars().count() > 240)
            })
        {
            return Err("task hints exceed routing bounds");
        }
        Ok(())
    }

    /// Only adds restrictions from explicit task wording. It never asserts
    /// resource availability or grants permission to write or publish.
    pub fn narrow_from_prompt(mut self, prompt: &str) -> Self {
        let lower = prompt.to_lowercase();
        if self.action.is_none() {
            let words: Vec<_> = lower.split_whitespace().take(4).collect();
            let verb = match words.as_slice() {
                ["please", verb, ..] | ["can" | "could" | "would", "you", verb, ..] => Some(*verb),
                [verb, ..] => Some(*verb),
                _ => None,
            };
            if let Some(verb) = verb
                && [
                    "read",
                    "inspect",
                    "review",
                    "draft",
                    "write",
                    "create",
                    "edit",
                    "prepare",
                    "execute",
                    "publish",
                    "send",
                    "rewrite",
                    "overwrite",
                    "delete",
                    "analyze",
                    "summarize",
                    "convert",
                ]
                .contains(&verb)
            {
                self.action = Some(verb.into());
            }
        }
        if [
            "read-only",
            "read only",
            "without changing",
            "without modifying",
            "do not change",
            "don't change",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
        {
            self.maximum_effect = Some(RoutingEffect::ReadOnly);
        }
        self
    }
}

/// Evaluation seam; native controls always use Full. Policy and loading gates
/// remain in force for every condition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RoutingAblation {
    DescriptorsWithStandardScorer,
    LexicalWithoutContracts,
    #[default]
    Full,
}
