#![forbid(unsafe_code)]
//! Fixture consumers and deterministic fakes. Production crates must not depend here.

mod compare;
mod conformance;
mod fake;
mod fixture;
mod normalize;
mod session_store;

pub use compare::{Comparison, ComparisonError, compare};
pub use conformance::{
    BoundedEventSink, ConformanceError, assert_cancellation, assert_harness_event_order,
    assert_json_round_trip, assert_provider_stream_contract, assert_secret_canary_absent,
    assert_tool_call_result_linkage,
};
pub use fake::{
    CancellationProbe, DeterministicIds, FakeClock, FakeFilesystemDescriptor,
    FakePermissionChannel, FakeProviderSession, FakeProviderStream, ScriptedProviderResponse,
};
pub use fixture::{
    ComparisonClass, FixtureCorpus, FixtureError, FixtureEvent, FixtureManifest, FixtureResult,
    ScenarioFixture, fixture_root,
};
pub use normalize::{NormalizationError, NormalizationRule, normalize_json};
pub use session_store::{
    AgentVesperReadStoreBuilder, FileTreeEntry, FileTreeHashManifest, LegacyStoreBuilder,
    NoWriteAssertion, SessionFixtureLoader, TemporaryReadStore, TestStoreError,
};
