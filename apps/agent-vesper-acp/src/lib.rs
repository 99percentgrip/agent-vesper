#![forbid(unsafe_code)]
//! Thin composition shared by the release binary and process-only conformance driver.

use std::{path::PathBuf, sync::Arc};

use vesper_acp::{AcpAdapter, AcpAdapterConfig};
use vesper_config::{PathEnvironment, Platform, ProfileName, VesperPaths};
use vesper_domain::{EndpointId, ModelId, ProviderId, QualifiedModelId};
use vesper_provider::{ProviderConfiguration, ProviderFactory};
use vesper_provider_glm::{GlmFactory, provider_id};
use vesper_provider_synthetic::SyntheticFactory;
use vesper_runtime::{
    ProviderRegistry, RuntimeDefaults, RuntimeSessionReads, RuntimeSessionWrites, RuntimeSupervisor,
};
use vesper_sessions::{
    AgentVesperSessionLayout, CompatibilityAvailability, CompositeSessionRepository,
    DiscoveryBounds, EmptySessionRepository, FilesystemSessionStore, LegacyDecodeBounds,
    LegacySessionLayout, SessionRepository, SessionSource, VesperDecodeBounds, VesperSessionWriter,
    WriteBounds,
};

/// Runs ACP stdio with an injected provider factory.
///
/// The release binary selects the factory from `AGENT_VESPER_PROVIDER` or the
/// `--provider` CLI flag (see [`run`] and [`boot`]); the non-default
/// process-test driver may wrap a selected factory with generic synchronization
/// only. The provider configuration, model, and default endpoint are resolved
/// from the factory's own identity via [`ProviderProfile`], keeping the
/// runtime provider-neutral.
pub async fn run_with_factory<F>(factory: F) -> Result<(), ()>
where
    F: ProviderFactory + 'static,
    F::Session: 'static,
{
    let providers = Arc::new(ProviderRegistry::new());
    let provider = factory.provider_id().clone();
    providers.register(factory).await.map_err(|_| ())?;

    let profile = ProviderProfile::for_identity(&provider)?;
    let qualified_model = runtime_model(&profile.model, &provider);
    let session_reads = session_reads_from_environment(&qualified_model).map_err(|_| ())?;
    let session_writes = session_writes_from_environment().map_err(|_| ())?;
    let runtime = RuntimeSupervisor::new(
        providers,
        RuntimeDefaults {
            provider_configuration: profile.provider_configuration,
            model: qualified_model,
            endpoint: profile.endpoint,
            system_instructions: Vec::new(),
            reasoning: None,
            sampling: None,
            maximum_output_tokens: None,
        },
    );
    let runtime = match session_reads {
        Some(reads) => runtime.with_session_reads(Arc::new(reads)),
        None => runtime,
    };
    let runtime = match session_writes {
        Some(writes) => runtime.with_session_writes(writes),
        None => runtime,
    };
    let runtime = Arc::new(runtime);
    AcpAdapter::new(runtime, AcpAdapterConfig::default())
        .run_stdio()
        .await
        .map_err(|_| ())
}

fn runtime_model(model: &ModelId, provider: &ProviderId) -> QualifiedModelId {
    QualifiedModelId {
        provider_id: provider.clone(),
        model_id: model.clone(),
    }
}

/// Resolved provider configuration, model, and default endpoint for one boot.
///
/// The runtime is provider-neutral; the composition boundary supplies the
/// concrete provider configuration, qualified model, and default endpoint so
/// freshly created sessions carry a stable, persistable endpoint identity.
/// GLM credentials and endpoint overrides are consulted only when the GLM
/// adapter is selected; the synthetic adapter needs no secret and never touches
/// GLM credential resolution.
struct ProviderProfile {
    provider_configuration: ProviderConfiguration,
    model: ModelId,
    endpoint: EndpointId,
}

impl ProviderProfile {
    /// Resolves the profile for a concrete provider identity, failing closed
    /// for any unrecognised adapter.
    fn for_identity(provider: &ProviderId) -> Result<Self, ()> {
        if provider == &provider_id() {
            // GLM (zai): the production adapter. Endpoint and credential
            // overrides are only consulted here.
            let mut provider_configuration = GlmFactory::default_configuration();
            if let Ok(base_url) = std::env::var("AGENT_VESPER_GLM_BASE_URL") {
                let allow_insecure =
                    std::env::var_os("AGENT_VESPER_ALLOW_INSECURE_LOOPBACK").is_some();
                provider_configuration
                    .values
                    .values
                    .insert("zai:endpoint-plan", serde_json::json!("custom"))
                    .map_err(|_| ())?;
                provider_configuration
                    .values
                    .values
                    .insert("zai:base-url", serde_json::json!(base_url))
                    .map_err(|_| ())?;
                provider_configuration
                    .values
                    .values
                    .insert("zai:allow-insecure-http", serde_json::json!(allow_insecure))
                    .map_err(|_| ())?;
                provider_configuration
                    .values
                    .values
                    .insert("zai:attach-inference-auth", serde_json::json!(true))
                    .map_err(|_| ())?;
            }
            let model = ModelId::new(
                std::env::var("AGENT_VESPER_GLM_MODEL").unwrap_or_else(|_| "glm-5.2".into()),
            )
            .map_err(|_| ())?;
            let endpoint = EndpointId::new("zai-coding").map_err(|_| ())?;
            Ok(Self {
                provider_configuration,
                model,
                endpoint,
            })
        } else if provider == &vesper_provider_synthetic::provider_id() {
            // Synthetic: deterministic in-process reference adapter. No
            // credential, no endpoint override, no network dependency.
            Ok(Self {
                provider_configuration: SyntheticFactory::default_configuration(),
                model: ModelId::new("synthetic-1").map_err(|_| ())?,
                endpoint: EndpointId::new("synthetic").map_err(|_| ())?,
            })
        } else {
            Err(())
        }
    }
}

fn session_reads_from_environment(
    model: &QualifiedModelId,
) -> Result<Option<RuntimeSessionReads>, ()> {
    let settings = SessionReadSettings::from_environment()?;
    if !settings.enable_vesper && !settings.enable_legacy {
        return Ok(None);
    }
    settings.build(model)
}

/// Resolves the optional transactional session writer from environment
/// configuration. The writer is only constructed when persistence is opted in
/// via `AGENT_VESPER_ENABLE_SESSION_WRITES`. All filesystem mutation stays
/// owned by `VesperSessionWriter` inside `vesper-sessions`; the composition
/// binary constructs and injects it without performing any I/O itself.
///
/// Write root resolution order: explicit `AGENT_VESPER_SESSION_WRITE_ROOT`,
/// then the shared read root `AGENT_VESPER_SESSION_ROOT`, then the platform
/// Agent Vesper data root. The writer requires an absolute root whose parent
/// exists; deployment is responsible for that parent when the default is used.
fn session_writes_from_environment() -> Result<Option<Arc<RuntimeSessionWrites>>, ()> {
    if !enabled("AGENT_VESPER_ENABLE_SESSION_WRITES") {
        return Ok(None);
    }
    let home = std::env::var_os(home_variable()).map(PathBuf::from);
    let root = match std::env::var_os("AGENT_VESPER_SESSION_WRITE_ROOT").map(PathBuf::from) {
        Some(root) => root,
        None => match std::env::var_os("AGENT_VESPER_SESSION_ROOT").map(PathBuf::from) {
            Some(root) => root,
            None => default_vesper_root(home.as_deref())?,
        },
    };
    let max_session_bytes =
        bounded_number::<u64>("AGENT_VESPER_SESSION_WRITE_MAX_BYTES", 16 * 1024 * 1024)?;
    let max_session_bytes = usize::try_from(max_session_bytes).map_err(|_| ())?;
    let bounds = WriteBounds {
        max_session_bytes,
        ..WriteBounds::default()
    };
    let writer =
        VesperSessionWriter::new(root, SessionSource::AgentVesper, bounds).map_err(|_| ())?;
    Ok(Some(Arc::new(RuntimeSessionWrites::new(Arc::new(writer)))))
}

#[derive(Debug, Clone)]
struct SessionReadSettings {
    enable_vesper: bool,
    enable_legacy: bool,
    vesper_root: Option<PathBuf>,
    legacy_root: Option<PathBuf>,
    legacy_profile: Option<ProfileName>,
    home: Option<PathBuf>,
    max_session_bytes: u64,
    max_entries: usize,
}

impl SessionReadSettings {
    fn from_environment() -> Result<Self, ()> {
        Ok(Self {
            enable_vesper: enabled("AGENT_VESPER_ENABLE_SESSION_READS")
                || enabled("AGENT_VESPER_ENABLE_VESPER_SESSION_READS"),
            enable_legacy: enabled("AGENT_VESPER_ENABLE_LEGACY_SESSION_READS"),
            vesper_root: std::env::var_os("AGENT_VESPER_SESSION_ROOT").map(PathBuf::from),
            legacy_root: std::env::var_os("AGENT_VESPER_LEGACY_SESSION_ROOT").map(PathBuf::from),
            legacy_profile: std::env::var("AGENT_VESPER_LEGACY_PROFILE")
                .ok()
                .map(ProfileName::new)
                .transpose()
                .map_err(|_| ())?,
            home: std::env::var_os(home_variable()).map(PathBuf::from),
            max_session_bytes: bounded_number("AGENT_VESPER_SESSION_MAX_BYTES", 16 * 1024 * 1024)?,
            max_entries: bounded_number("AGENT_VESPER_SESSION_MAX_ENTRIES", 10_000)?,
        })
    }

    fn build(self, model: &QualifiedModelId) -> Result<Option<RuntimeSessionReads>, ()> {
        let bounds = DiscoveryBounds {
            max_entries: self.max_entries,
            max_session_bytes: self.max_session_bytes,
            ..DiscoveryBounds::default()
        };
        let memory: Arc<dyn SessionRepository> =
            Arc::new(EmptySessionRepository::new(SessionSource::InMemory).map_err(|_| ())?);
        let agent: Arc<dyn SessionRepository> = if self.enable_vesper {
            let root = match self.vesper_root {
                Some(root) => root,
                None => default_vesper_root(self.home.as_deref())?,
            };
            Arc::new(
                FilesystemSessionStore::new(root, SessionSource::AgentVesper, bounds)
                    .map_err(|_| ())?,
            )
        } else {
            Arc::new(EmptySessionRepository::new(SessionSource::AgentVesper).map_err(|_| ())?)
        };
        let legacy_source = SessionSource::LegacyNativeGlm {
            profile: self
                .legacy_profile
                .as_ref()
                .map(|profile| profile.as_str().to_owned()),
        };
        let legacy: Arc<dyn SessionRepository> = if self.enable_legacy {
            let root = match self.legacy_root {
                Some(root) => root,
                None => default_legacy_root(
                    self.home.as_deref().ok_or(())?,
                    self.legacy_profile.as_ref(),
                ),
            };
            Arc::new(
                FilesystemSessionStore::new(root, legacy_source.clone(), bounds).map_err(|_| ())?,
            )
        } else {
            Arc::new(EmptySessionRepository::new(legacy_source).map_err(|_| ())?)
        };
        let repository =
            Arc::new(CompositeSessionRepository::new(memory, agent, legacy).map_err(|_| ())?);
        let endpoint = EndpointId::new("zai-coding").map_err(|_| ())?;
        let mut availability = CompatibilityAvailability::default()
            .with_provider(model.provider_id.clone())
            .with_model(model.clone());
        for endpoint in [
            endpoint,
            EndpointId::new("zai-standard").map_err(|_| ())?,
            EndpointId::new("zai-bigmodel-cn").map_err(|_| ())?,
            EndpointId::new("zai-custom").map_err(|_| ())?,
        ] {
            availability = availability.with_endpoint(model.provider_id.clone(), endpoint);
        }
        Ok(Some(RuntimeSessionReads::new(
            repository,
            availability,
            LegacyDecodeBounds {
                max_file_bytes: usize::try_from(self.max_session_bytes).map_err(|_| ())?,
                ..LegacyDecodeBounds::default()
            },
            VesperDecodeBounds {
                max_file_bytes: usize::try_from(self.max_session_bytes).map_err(|_| ())?,
                ..VesperDecodeBounds::default()
            },
        )))
    }
}

fn enabled(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

fn bounded_number<T>(name: &str, default: T) -> Result<T, ()>
where
    T: std::str::FromStr + PartialEq + Default,
{
    let value = std::env::var(name)
        .ok()
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| ())?
        .unwrap_or(default);
    if value == T::default() {
        return Err(());
    }
    Ok(value)
}

fn default_vesper_root(home: Option<&std::path::Path>) -> Result<PathBuf, ()> {
    let environment = PathEnvironment {
        home: home.map(std::path::Path::to_path_buf),
        xdg_data_home: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        app_data: std::env::var_os("APPDATA").map(PathBuf::from),
        local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        ..PathEnvironment::default()
    };
    let paths = VesperPaths::resolve(current_platform(), &environment).map_err(|_| ())?;
    Ok(AgentVesperSessionLayout::from_paths(&paths)
        .root()
        .to_path_buf())
}

fn default_legacy_root(home: &std::path::Path, profile: Option<&ProfileName>) -> PathBuf {
    profile
        .map_or_else(
            || LegacySessionLayout::default_profile(home),
            |profile| LegacySessionLayout::named_profile(home, profile.clone()),
        )
        .root()
        .to_path_buf()
}

const fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

const fn home_variable() -> &'static str {
    if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    }
}

/// Runs the normal release composition with the provider selected by
/// `AGENT_VESPER_PROVIDER` (default `glm`).
pub async fn run() -> Result<(), ()> {
    boot(&selected_provider_token()).await
}

/// Boots the composition with an explicitly resolved provider token.
///
/// The composition boundary keeps the runtime provider-neutral: it maps a
/// provider token to the matching concrete factory. `glm`/`zai` boot the Z.ai
/// GLM adapter (the production default); `synthetic` boots the in-process
/// reference adapter with no network or credential dependency. Unknown tokens
/// fail closed with a startup error rather than an ambiguous default.
pub async fn boot(provider: &str) -> Result<(), ()> {
    match provider {
        "glm" | "zai" => run_with_factory(GlmFactory::default()).await,
        "synthetic" => run_with_factory(SyntheticFactory::default()).await,
        _ => Err(()),
    }
}

/// Resolves the provider token from `AGENT_VESPER_PROVIDER`, defaulting to
/// `glm` so the production adapter remains the default when unset.
fn selected_provider_token() -> String {
    std::env::var("AGENT_VESPER_PROVIDER").unwrap_or_else(|_| String::from("glm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> QualifiedModelId {
        runtime_model(&ModelId::new("glm-5.2").unwrap(), &provider_id())
    }

    #[test]
    fn explicit_missing_roots_configure_readers_without_creating_directories() {
        let base = std::env::temp_dir().join(format!(
            "agent-vesper-session-config-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let vesper_root = base.join("vesper");
        let legacy_root = base.join("legacy");
        let reads = SessionReadSettings {
            enable_vesper: true,
            enable_legacy: true,
            vesper_root: Some(vesper_root.clone()),
            legacy_root: Some(legacy_root.clone()),
            legacy_profile: None,
            home: None,
            max_session_bytes: 4096,
            max_entries: 10,
        }
        .build(&model())
        .unwrap();
        assert!(reads.is_some());
        assert!(!vesper_root.exists());
        assert!(!legacy_root.exists());
    }

    #[test]
    fn unsafe_relative_roots_fail_closed() {
        let result = SessionReadSettings {
            enable_vesper: true,
            enable_legacy: false,
            vesper_root: Some(PathBuf::from("relative/sessions")),
            legacy_root: None,
            legacy_profile: None,
            home: None,
            max_session_bytes: 4096,
            max_entries: 10,
        }
        .build(&model());
        assert!(result.is_err());
        assert!(!PathBuf::from("relative/sessions").exists());
    }
}
