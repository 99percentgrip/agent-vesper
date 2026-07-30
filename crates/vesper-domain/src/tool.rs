use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{BoundedString, ExtensionMap, ToolCallId, ToolId, ToolResultId};

/// Invalid stable/provider-facing tool name.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolNameError {
    /// Name is blank.
    #[error("tool name must not be blank")]
    Blank,
    /// Name exceeds the contract bound.
    #[error(transparent)]
    Bounded(#[from] crate::BoundedStringError),
}

/// Stable harness-side tool name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BoundedString<128>", into = "BoundedString<128>")]
pub struct HarnessToolName(BoundedString<128>);

impl HarnessToolName {
    /// Creates a bounded harness tool name.
    pub fn new(value: impl Into<String>) -> Result<Self, ToolNameError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ToolNameError::Blank);
        }
        BoundedString::new(value).map(Self).map_err(Into::into)
    }

    /// Returns the stable name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<BoundedString<128>> for HarnessToolName {
    type Error = ToolNameError;

    fn try_from(value: BoundedString<128>) -> Result<Self, Self::Error> {
        Self::new(value.as_str())
    }
}

impl From<HarnessToolName> for BoundedString<128> {
    fn from(value: HarnessToolName) -> Self {
        value.0
    }
}

/// Adapter-facing tool name after dialect mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BoundedString<128>", into = "BoundedString<128>")]
pub struct ProviderToolName(BoundedString<128>);

impl ProviderToolName {
    /// Creates a bounded provider-facing name.
    pub fn new(value: impl Into<String>) -> Result<Self, ToolNameError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ToolNameError::Blank);
        }
        BoundedString::new(value).map(Self).map_err(Into::into)
    }

    /// Returns the adapter-facing name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<BoundedString<128>> for ProviderToolName {
    type Error = ToolNameError;

    fn try_from(value: BoundedString<128>) -> Result<Self, Self::Error> {
        Self::new(value.as_str())
    }
}

impl From<ProviderToolName> for BoundedString<128> {
    fn from(value: ProviderToolName) -> Self {
        value.0
    }
}

/// Provider-neutral tool selection intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "tool", rename_all = "kebab-case")]
pub enum ToolChoiceIntent {
    /// Provider selects.
    Auto,
    /// No tool may be selected.
    None,
    /// At least one tool is required.
    Required,
    /// Select one stable harness tool.
    Named(ToolId),
    /// Provider extension interpreted only by its adapter.
    ProviderExtension(String),
}

/// Execution authority class used by policy before an executor exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolExecutionClass {
    /// No state mutation.
    ReadOnly,
    /// Scoped mutation.
    Mutating,
    /// Explicit shell interpretation.
    Shell,
    /// Process execution without shell interpretation.
    Process,
    /// External or nested workflow.
    NestedWorkflow,
}

/// Provider-neutral tool definition. Adapters own schema-dialect conversion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Stable harness tool ID.
    pub id: ToolId,
    /// Stable harness name.
    pub harness_name: HarnessToolName,
    /// Optional provider-facing name selected by an adapter.
    pub provider_name: Option<ProviderToolName>,
    /// User/model-facing description.
    pub description: String,
    /// Normalized JSON Schema input.
    pub input_schema: Value,
    /// Authority classification.
    pub execution_class: ToolExecutionClass,
    /// Namespaced extension metadata.
    #[serde(default)]
    pub extensions: ExtensionMap,
}

/// One normalized tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Call/result linkage identifier.
    pub id: ToolCallId,
    /// Harness tool identifier.
    pub tool_id: ToolId,
    /// Parsed normalized arguments.
    pub arguments: Value,
    /// Provider data preserved for adapter round trips.
    #[serde(default)]
    pub extensions: ExtensionMap,
}

/// Stable assembly identity for fragmented and parallel tool-call streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentedToolCallIdentity {
    /// Provider stream index; equal indexes identify fragments of one call.
    pub stream_index: u32,
    /// Stable call ID once available.
    pub call_id: Option<ToolCallId>,
    /// Adapter-selected provider tool name once available.
    pub provider_name: Option<ProviderToolName>,
}

/// Completed JSON arguments after fragmented assembly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletedToolArguments {
    /// Linked call.
    pub call_id: ToolCallId,
    /// Parsed JSON value.
    pub value: Value,
    /// Exact assembled JSON text for compatibility diagnostics.
    pub assembled_json: String,
}

/// Tool result status independent of frontend rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolResultStatus {
    /// Successful result.
    Succeeded,
    /// Tool execution failed.
    Failed,
    /// Cancelled before completion.
    Cancelled,
    /// Policy rejected execution.
    Denied,
}

/// Structured source location optionally attached to a result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredLocation {
    /// Authority-scoped path or logical URI.
    pub reference: String,
    /// One-based line.
    pub line: Option<u32>,
    /// One-based column.
    pub column: Option<u32>,
}

/// Bounded user-display summary of changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Files changed.
    pub files_changed: u32,
    /// Added lines.
    pub additions: u64,
    /// Removed lines.
    pub deletions: u64,
}

/// Tool output paired to exactly one call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// Stable result identity.
    pub id: ToolResultId,
    /// Call/result linkage identifier.
    pub call_id: ToolCallId,
    /// Structured or textual normalized output.
    pub output: Value,
    /// Terminal result status.
    pub status: ToolResultStatus,
    /// Optional structured locations.
    #[serde(default)]
    pub locations: Vec<StructuredLocation>,
    /// Optional bounded diff summary.
    pub diff_summary: Option<DiffSummary>,
    /// Namespaced extension metadata.
    #[serde(default)]
    pub extensions: ExtensionMap,
}
