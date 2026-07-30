use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Comparison strictness declared by a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComparisonClass {
    /// Canonical value equality.
    ExactOutput,
    /// Named semantics and event order.
    SemanticParity,
    /// Reader/schema compatibility.
    SchemaCompatibility,
    /// Rust may strengthen but never weaken.
    SecurityInvariant,
    /// Controlled distribution comparison.
    Performance,
}

/// Versioned scenario manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureManifest {
    /// Stable scenario identity.
    pub scenario_id: String,
    /// Schema version.
    pub schema_version: u32,
    /// Fixture category.
    pub category: String,
    /// Comparison class.
    pub comparison_class: ComparisonClass,
    /// Frozen Python commit.
    pub source_commit: String,
    /// Platform requirements.
    pub platform_requirements: Value,
    /// Scenario input.
    pub input: Value,
    /// Isolated environment.
    pub environment: Value,
    /// Referenced fixture files.
    pub fixture_files: Vec<String>,
    /// Ordered expected events.
    pub expected_events: Vec<Value>,
    /// Expected final state.
    pub expected_state: Value,
    /// Expected persistence observations.
    pub expected_persistence: Value,
    /// Expected process observations.
    pub expected_process_observations: Value,
    /// Expected network observations.
    pub expected_network_observations: Value,
    /// Approved normalization rules.
    pub normalization_rules: Vec<Value>,
    /// Security assertions.
    pub security_assertions: Vec<Value>,
    /// Timeout contract.
    pub timeout: Value,
}

/// Canonical fixture event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureEvent {
    /// Monotonic sequence.
    pub seq: u64,
    /// Event kind.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Event data.
    pub data: Value,
}

/// Versioned result envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureResult {
    /// Scenario identity.
    pub scenario_id: String,
    /// Runner identity.
    pub runner: String,
    /// Runner version.
    pub runner_version: String,
    /// Platform descriptor.
    pub platform: Value,
    /// Ordered events.
    pub events: Vec<FixtureEvent>,
    /// Final state.
    pub final_state: Value,
    /// Persisted paths.
    pub persisted_files: Vec<String>,
    /// Exact file hashes.
    pub file_hashes: BTreeMap<String, String>,
    /// File mode/ACL observations.
    pub file_modes_or_acl_status: BTreeMap<String, String>,
    /// Process observations.
    pub process_observations: Value,
    /// Network observations.
    pub network_observations: Value,
    /// Sanitized logs.
    pub logs: Vec<String>,
    /// Redaction assertions.
    pub redaction_assertions: Vec<Value>,
    /// Duration metadata.
    pub duration_metadata: Value,
    /// Classified result.
    pub result: Value,
}

/// Loaded manifest/result pair.
#[derive(Debug, Clone)]
pub struct ScenarioFixture {
    /// Scenario directory.
    pub directory: PathBuf,
    /// Manifest.
    pub manifest: FixtureManifest,
    /// Python oracle result.
    pub result: FixtureResult,
    /// Raw manifest JSON.
    pub manifest_json: Value,
    /// Raw result JSON.
    pub result_json: Value,
}

/// Entire authoritative corpus.
#[derive(Debug, Clone)]
pub struct FixtureCorpus {
    /// Fixture root.
    pub root: PathBuf,
    /// Stable scenario-ID order.
    pub scenarios: Vec<ScenarioFixture>,
}

impl FixtureCorpus {
    /// Loads and schema-validates every manifest/result pair.
    pub fn load(root: impl Into<PathBuf>) -> Result<Self, FixtureError> {
        let root = root.into();
        let manifest_schema = read_json(&root.join("schema/scenario-manifest-v1.schema.json"))?;
        let result_schema = read_json(&root.join("schema/result-v1.schema.json"))?;
        let manifest_validator = jsonschema::validator_for(&manifest_schema)
            .map_err(|error| FixtureError::SchemaCompile(error.to_string()))?;
        let result_validator = jsonschema::validator_for(&result_schema)
            .map_err(|error| FixtureError::SchemaCompile(error.to_string()))?;

        let mut manifests = Vec::new();
        collect_named(&root, "manifest.json", &mut manifests)?;
        manifests.sort();
        let mut scenarios = Vec::with_capacity(manifests.len());
        let mut ids = BTreeSet::new();
        for manifest_path in manifests {
            let directory = manifest_path
                .parent()
                .ok_or_else(|| FixtureError::Layout(manifest_path.clone()))?
                .to_path_buf();
            let result_path = directory.join("result.python.json");
            let manifest_json = read_json(&manifest_path)?;
            let result_json = read_json(&result_path)?;
            validate(&manifest_validator, &manifest_json, manifest_path.as_path())?;
            validate(&result_validator, &result_json, result_path.as_path())?;
            let manifest: FixtureManifest =
                serde_json::from_value(manifest_json.clone()).map_err(|source| {
                    FixtureError::Json {
                        path: manifest_path.clone(),
                        source,
                    }
                })?;
            let result: FixtureResult =
                serde_json::from_value(result_json.clone()).map_err(|source| {
                    FixtureError::Json {
                        path: result_path,
                        source,
                    }
                })?;
            if manifest.scenario_id != result.scenario_id {
                return Err(FixtureError::ScenarioMismatch(manifest.scenario_id));
            }
            if !ids.insert(manifest.scenario_id.clone()) {
                return Err(FixtureError::DuplicateScenario(manifest.scenario_id));
            }
            assert_event_order(&result)?;
            assert_canary_clean(&result_json)?;
            scenarios.push(ScenarioFixture {
                directory,
                manifest,
                result,
                manifest_json,
                result_json,
            });
        }
        scenarios.sort_by(|left, right| left.manifest.scenario_id.cmp(&right.manifest.scenario_id));
        Ok(Self { root, scenarios })
    }

    /// Verifies every indexed payload and rejects index omissions/additions.
    pub fn verify_index(&self) -> Result<usize, FixtureError> {
        let index_path = self.root.join("manifest-sha256.json");
        let index: HashIndex =
            serde_json::from_value(read_json(&index_path)?).map_err(|source| {
                FixtureError::Json {
                    path: index_path,
                    source,
                }
            })?;
        let mut actual_paths = BTreeSet::new();
        for relative in index.files.keys() {
            actual_paths.insert(relative.clone());
        }
        let expected_paths = indexed_payload_paths(&self.root)?;
        if actual_paths != expected_paths {
            return Err(FixtureError::IndexSetMismatch);
        }
        for (relative, expected) in &index.files {
            let path = self
                .root
                .parent()
                .ok_or_else(|| FixtureError::Layout(self.root.clone()))?
                .join(relative);
            let bytes = fs::read(&path).map_err(|source| FixtureError::Io {
                path: path.clone(),
                source,
            })?;
            let actual = Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if &actual != expected {
                return Err(FixtureError::HashMismatch {
                    path,
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        Ok(index.files.len())
    }

    /// Returns category counts.
    #[must_use]
    pub fn category_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for scenario in &self.scenarios {
            *counts
                .entry(scenario.manifest.category.clone())
                .or_default() += 1;
        }
        counts
    }

    /// Finds one scenario by stable identity.
    #[must_use]
    pub fn scenario(&self, scenario_id: &str) -> Option<&ScenarioFixture> {
        self.scenarios
            .binary_search_by_key(&scenario_id, |scenario| {
                scenario.manifest.scenario_id.as_str()
            })
            .ok()
            .map(|index| &self.scenarios[index])
    }

    /// Returns the SHA-256 of the canonical fixture index file itself.
    pub fn index_sha256(&self) -> Result<String, FixtureError> {
        let bytes = fs::read(self.root.join("manifest-sha256.json")).map_err(|source| {
            FixtureError::Io {
                path: self.root.join("manifest-sha256.json"),
                source,
            }
        })?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}

#[derive(Debug, Deserialize)]
struct HashIndex {
    files: BTreeMap<String, String>,
}

fn read_json(path: &Path) -> Result<Value, FixtureError> {
    let bytes = fs::read(path).map_err(|source| FixtureError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| FixtureError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn validate(
    validator: &jsonschema::Validator,
    instance: &Value,
    path: &Path,
) -> Result<(), FixtureError> {
    validator
        .validate(instance)
        .map_err(|error| FixtureError::Validation {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

fn collect_named(root: &Path, name: &str, output: &mut Vec<PathBuf>) -> Result<(), FixtureError> {
    for entry in fs::read_dir(root).map_err(|source| FixtureError::Io {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| FixtureError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_named(&path, name, output)?;
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            output.push(path);
        }
    }
    Ok(())
}

fn indexed_payload_paths(root: &Path) -> Result<BTreeSet<String>, FixtureError> {
    let repository = root
        .parent()
        .ok_or_else(|| FixtureError::Layout(root.to_path_buf()))?;
    let mut paths = BTreeSet::new();
    let mut candidates = Vec::new();
    collect_payloads(root, &mut candidates)?;
    for path in candidates {
        let relative = path
            .strip_prefix(repository)
            .map_err(|_| FixtureError::Layout(path.clone()))?;
        paths.insert(relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(paths)
}

fn collect_payloads(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), FixtureError> {
    for entry in fs::read_dir(root).map_err(|source| FixtureError::Io {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| FixtureError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_payloads(&path, output)?;
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if name == "manifest.json"
            || name == "result.python.json"
            || (path
                .parent()
                .is_some_and(|parent| parent.ends_with("schema"))
                && name.ends_with(".schema.json"))
        {
            output.push(path);
        }
    }
    Ok(())
}

fn assert_event_order(result: &FixtureResult) -> Result<(), FixtureError> {
    for (expected, event) in result.events.iter().enumerate() {
        if event.seq != u64::try_from(expected).unwrap_or(u64::MAX) {
            return Err(FixtureError::EventOrder {
                scenario: result.scenario_id.clone(),
                expected,
                actual: event.seq,
            });
        }
    }
    Ok(())
}

fn assert_canary_clean(value: &Value) -> Result<(), FixtureError> {
    let encoded = serde_json::to_string(value).map_err(|source| FixtureError::Json {
        path: PathBuf::from("<in-memory-result>"),
        source,
    })?;
    if encoded.contains("VESPER_SECRET_CANARY_7xQ9m2Kp") {
        return Err(FixtureError::CanaryLeak);
    }
    Ok(())
}

/// Returns the repository fixture root.
#[must_use]
pub fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Fixture loading/validation failure.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// Filesystem error.
    #[error("fixture I/O failed at {path}: {source}")]
    Io {
        /// Path.
        path: PathBuf,
        /// Source.
        source: std::io::Error,
    },
    /// JSON parse/type error.
    #[error("fixture JSON failed at {path}: {source}")]
    Json {
        /// Path.
        path: PathBuf,
        /// Source.
        source: serde_json::Error,
    },
    /// JSON Schema compile error.
    #[error("fixture schema failed to compile: {0}")]
    SchemaCompile(String),
    /// Instance validation error.
    #[error("fixture schema validation failed at {path}: {message}")]
    Validation {
        /// Path.
        path: PathBuf,
        /// Safe validation message.
        message: String,
    },
    /// Invalid directory layout.
    #[error("fixture layout is invalid at {0}")]
    Layout(PathBuf),
    /// Manifest/result IDs differ.
    #[error("fixture scenario ID mismatch for {0}")]
    ScenarioMismatch(String),
    /// Duplicate ID.
    #[error("duplicate fixture scenario {0}")]
    DuplicateScenario(String),
    /// Event ordering changed.
    #[error("event order mismatch in {scenario}: expected {expected}, got {actual}")]
    EventOrder {
        /// Scenario.
        scenario: String,
        /// Expected sequence.
        expected: usize,
        /// Actual sequence.
        actual: u64,
    },
    /// Raw canary leaked.
    #[error("fixture result contains the raw secret canary")]
    CanaryLeak,
    /// Index does not cover exactly the canonical payload set.
    #[error("fixture hash index file set does not match canonical payloads")]
    IndexSetMismatch,
    /// Indexed payload hash changed.
    #[error("fixture hash mismatch at {path}: expected {expected}, got {actual}")]
    HashMismatch {
        /// Path.
        path: PathBuf,
        /// Expected.
        expected: String,
        /// Actual.
        actual: String,
    },
}

#[cfg(test)]
mod tests {
    use vesper_domain::{LegacySessionError, LegacySessionV1};

    use super::*;

    #[test]
    fn all_authoritative_scenarios_validate_and_index_matches() {
        let corpus = FixtureCorpus::load(fixture_root()).unwrap();
        assert_eq!(corpus.scenarios.len(), 76);
        assert_eq!(corpus.verify_index().unwrap(), 154);
        assert_eq!(corpus.category_counts().values().sum::<usize>(), 76);
        assert_eq!(
            corpus.index_sha256().unwrap(),
            "d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a"
        );
    }

    #[test]
    fn all_seven_session_fixture_outcomes_have_compatibility_coverage() {
        let corpus = FixtureCorpus::load(fixture_root()).unwrap();
        for scenario_id in [
            "session.schema1-complete",
            "session.minimal-legacy",
            "session.replay-and-lineage",
            "session.reasoning-enabled",
            "session.reasoning-disabled",
        ] {
            let fixture = corpus.scenario(scenario_id).unwrap();
            let bytes = serde_json::to_vec(&fixture.result.final_state).unwrap();
            let record = LegacySessionV1::decode_json(&bytes).unwrap();
            assert_eq!(
                serde_json::to_value(&record).unwrap(),
                fixture.result.final_state,
                "known-field loss for {scenario_id}"
            );
            let round_trip = LegacySessionV1::decode_json(&record.encode_json().unwrap()).unwrap();
            assert_eq!(record, round_trip, "{scenario_id}");
        }

        let unknown = corpus.scenario("session.unknown-fields").unwrap();
        assert_eq!(
            unknown.result.final_state["unknown_accepted"],
            Value::Bool(true)
        );
        let synthetic =
            LegacySessionV1::decode_json(br#"{"future_field":{"preserve":true}}"#).unwrap();
        assert!(synthetic.unknown_fields.contains_key("future_field"));

        let corrupt = corpus.scenario("session.corrupt-json").unwrap();
        assert!(corrupt.result.final_state["loaded"].is_null());
        assert_eq!(
            LegacySessionV1::decode_json(b"{broken"),
            Err(LegacySessionError::MalformedJson)
        );
    }
}
