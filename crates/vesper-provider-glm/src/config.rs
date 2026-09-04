use std::{fmt, time::Duration};

use serde::{Deserialize, Serialize};
use url::Url;
use vesper_domain::{EndpointId, ModelId, SchemaVersion};
use vesper_provider::ProviderConfiguration;
use vesper_security::{EndpointTrust, RedactedUrl};

use crate::{GlmAdapterError, catalog::model_supports_plan, provider_id};

/// Frozen Z.ai endpoint-plan choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GlmPlan {
    /// Z.ai Coding Plan endpoint.
    Coding,
    /// Z.ai pay-as-you-go endpoint.
    Standard,
    /// BigModel China endpoint.
    BigModel,
    /// Explicit user-configured endpoint.
    Custom,
}

impl GlmPlan {
    /// Stable endpoint identity.
    pub fn endpoint_id(self) -> EndpointId {
        EndpointId::new(match self {
            Self::Coding => "zai-coding",
            Self::Standard => "zai-standard",
            Self::BigModel => "zai-bigmodel-cn",
            Self::Custom => "zai-custom",
        })
        .expect("static endpoint ID")
    }

    /// Frozen legacy plan key.
    #[must_use]
    pub const fn legacy_key(self) -> &'static str {
        match self {
            Self::Coding => "coding",
            Self::Standard => "standard",
            Self::BigModel => "bigmodel",
            Self::Custom => "custom",
        }
    }
}

/// Parsed endpoint with explicit trust and credential behavior.
#[derive(Clone)]
pub struct GlmEndpoint {
    plan: GlmPlan,
    base_url: Url,
    redacted: RedactedUrl,
    trust: EndpointTrust,
    attach_inference_auth: bool,
    quota_supported: bool,
}

impl fmt::Debug for GlmEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GlmEndpoint")
            .field("plan", &self.plan)
            .field("base_url", &self.redacted)
            .field("trust", &self.trust)
            .field("attach_inference_auth", &self.attach_inference_auth)
            .field("quota_supported", &self.quota_supported)
            .finish()
    }
}

impl GlmEndpoint {
    /// Returns one frozen official endpoint.
    pub fn official(plan: GlmPlan) -> Result<Self, GlmAdapterError> {
        let value = match plan {
            GlmPlan::Coding => "https://api.z.ai/api/coding/paas/v4",
            GlmPlan::Standard => "https://api.z.ai/api/paas/v4",
            GlmPlan::BigModel => "https://open.bigmodel.cn/api/paas/v4",
            GlmPlan::Custom => {
                return Err(GlmAdapterError::Configuration(
                    "custom plan requires a custom URL",
                ));
            }
        };
        Self::from_parts(plan, value, false, true)
    }

    /// Creates an explicit custom endpoint.
    ///
    /// Plain HTTP requires an explicit development opt-in. Authentication is
    /// never inherited implicitly by a custom endpoint.
    pub fn custom(
        value: &str,
        allow_insecure_http: bool,
        attach_inference_auth: bool,
    ) -> Result<Self, GlmAdapterError> {
        Self::from_parts(
            GlmPlan::Custom,
            value,
            allow_insecure_http,
            attach_inference_auth,
        )
    }

    fn from_parts(
        plan: GlmPlan,
        value: &str,
        allow_insecure_http: bool,
        attach_inference_auth: bool,
    ) -> Result<Self, GlmAdapterError> {
        let parsed = Url::parse(value)
            .map_err(|_| GlmAdapterError::Configuration("endpoint URL is invalid"))?;
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(GlmAdapterError::Configuration(
                "endpoint must be an absolute URL without userinfo, query, or fragment",
            ));
        }
        match parsed.scheme() {
            "https" => {}
            "http" if allow_insecure_http && plan == GlmPlan::Custom => {}
            _ => {
                return Err(GlmAdapterError::Configuration(
                    "endpoint requires HTTPS unless insecure development mode is explicit",
                ));
            }
        }
        let official = official_identity(plan, &parsed);
        if plan != GlmPlan::Custom && !official {
            return Err(GlmAdapterError::Configuration(
                "official endpoint does not match its pinned URL",
            ));
        }
        let trust = if official {
            EndpointTrust::Official
        } else if parsed
            .host_str()
            .is_some_and(|host| host == "localhost" || host == "127.0.0.1" || host == "::1")
        {
            EndpointTrust::Local
        } else {
            EndpointTrust::ConfiguredRemote
        };
        let redacted = RedactedUrl::parse(parsed.as_str())
            .map_err(|_| GlmAdapterError::Configuration("endpoint URL cannot be redacted"))?;
        Ok(Self {
            plan,
            base_url: parsed,
            redacted,
            trust,
            attach_inference_auth: official || attach_inference_auth,
            quota_supported: official,
        })
    }

    /// Plan.
    #[must_use]
    pub const fn plan(&self) -> GlmPlan {
        self.plan
    }

    /// Stable endpoint ID.
    pub fn endpoint_id(&self) -> EndpointId {
        self.plan.endpoint_id()
    }

    /// Parsed base URL.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Redacted display URL.
    #[must_use]
    pub const fn redacted(&self) -> &RedactedUrl {
        &self.redacted
    }

    /// Trust classification.
    #[must_use]
    pub const fn trust(&self) -> EndpointTrust {
        self.trust
    }

    /// Whether inference authentication is intentionally attached.
    #[must_use]
    pub const fn attach_inference_auth(&self) -> bool {
        self.attach_inference_auth
    }

    /// Whether official quota monitoring is available.
    #[must_use]
    pub const fn quota_supported(&self) -> bool {
        self.quota_supported
    }

    #[cfg(test)]
    pub(crate) fn with_test_quota_support(mut self) -> Self {
        self.quota_supported = true;
        self
    }

    /// Chat-completions URL.
    pub fn chat_completions_url(&self) -> Result<Url, GlmAdapterError> {
        append_path(&self.base_url, "chat/completions")
    }

    /// Official quota URL. Custom endpoints never produce one.
    pub fn quota_url(&self) -> Result<Url, GlmAdapterError> {
        if !self.quota_supported {
            return Err(GlmAdapterError::UnsupportedRequest(
                "quota is unavailable for custom endpoints",
            ));
        }
        let origin = self.base_url.origin().ascii_serialization();
        Url::parse(&format!("{origin}/api/monitor/usage/quota/limit"))
            .map_err(|_| GlmAdapterError::Configuration("quota URL is invalid"))
    }
}

fn append_path(base: &Url, suffix: &str) -> Result<Url, GlmAdapterError> {
    let mut value = base.as_str().trim_end_matches('/').to_owned();
    value.push('/');
    value.push_str(suffix);
    Url::parse(&value).map_err(|_| GlmAdapterError::Configuration("request URL is invalid"))
}

fn official_identity(plan: GlmPlan, url: &Url) -> bool {
    let expected = match plan {
        GlmPlan::Coding => "https://api.z.ai/api/coding/paas/v4",
        GlmPlan::Standard => "https://api.z.ai/api/paas/v4",
        GlmPlan::BigModel => "https://open.bigmodel.cn/api/paas/v4",
        GlmPlan::Custom => return false,
    };
    url.as_str().trim_end_matches('/') == expected
}

/// Frozen GLM reasoning choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GlmReasoningMode {
    /// Thinking disabled.
    Disabled,
    /// Standard visible thinking.
    Enabled,
    /// Flagship-line high effort.
    High,
    /// Flagship-line maximum effort.
    Max,
}

impl GlmReasoningMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

/// Frozen sampling profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GlmGenerationProfile {
    /// Provider defaults.
    Balanced,
    /// Temperature 0.7.
    Precise,
    /// Top-p 0.98.
    Exploratory,
}

impl GlmGenerationProfile {
    /// Frozen profile controls.
    #[must_use]
    pub const fn controls(self) -> (Option<f64>, Option<f64>) {
        match self {
            Self::Balanced => (None, None),
            Self::Precise => (Some(0.7), None),
            Self::Exploratory => (None, Some(0.98)),
        }
    }
}

/// Production GLM adapter configuration.
#[derive(Debug, Clone)]
pub struct GlmConfig {
    /// Selected model.
    pub model: ModelId,
    /// Optional purpose-built auxiliary model used for compaction and other
    /// tool-free background inference. `None` routes those requests to the
    /// acting model.
    pub auxiliary_model: Option<ModelId>,
    /// Selected endpoint.
    pub endpoint: GlmEndpoint,
    /// Reasoning mode.
    pub reasoning: GlmReasoningMode,
    /// Generation profile.
    pub generation_profile: GlmGenerationProfile,
    /// Automatic continuation maximum, capped at 20.
    pub continuation_limit: u32,
    /// Connect timeout.
    pub connect_timeout: Duration,
    /// Absolute streaming-generation safety ceiling. Streaming activity does
    /// not reset this bound; `read_timeout` independently bounds inactivity.
    pub request_timeout: Duration,
    /// Read-inactivity timeout.
    pub read_timeout: Duration,
    /// Safe user agent.
    pub user_agent: String,
}

impl Default for GlmConfig {
    fn default() -> Self {
        Self {
            model: ModelId::new("glm-5.3").expect("static model ID"),
            auxiliary_model: None,
            endpoint: GlmEndpoint::official(GlmPlan::Coding).expect("static endpoint"),
            reasoning: GlmReasoningMode::Enabled,
            generation_profile: GlmGenerationProfile::Balanced,
            continuation_limit: 20,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30 * 60),
            read_timeout: Duration::from_secs(180),
            user_agent: format!("agent-vesper/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl GlmConfig {
    /// Validates the frozen model/plan and safe bounds.
    pub fn validate(&self) -> Result<(), GlmAdapterError> {
        if self.continuation_limit > 20 {
            return Err(GlmAdapterError::Configuration(
                "continuation limit exceeds frozen maximum",
            ));
        }
        if self.user_agent.is_empty()
            || self.user_agent.len() > 256
            || self.user_agent.chars().any(char::is_control)
        {
            return Err(GlmAdapterError::Configuration("user agent is invalid"));
        }
        if !model_supports_plan(self.model.as_str(), self.endpoint.plan()) {
            return if crate::catalog::is_known_model(self.model.as_str()) {
                Err(GlmAdapterError::ModelPlanMismatch)
            } else {
                Err(GlmAdapterError::UnknownModel)
            };
        }
        if let Some(auxiliary) = &self.auxiliary_model
            && !model_supports_plan(auxiliary.as_str(), self.endpoint.plan())
        {
            return if crate::catalog::is_known_model(auxiliary.as_str()) {
                Err(GlmAdapterError::ModelPlanMismatch)
            } else {
                Err(GlmAdapterError::UnknownModel)
            };
        }
        if !crate::GlmCatalog::supports_reasoning_mode(self.model.as_str(), self.reasoning.as_str())
        {
            return Err(GlmAdapterError::Configuration(
                "reasoning mode is unavailable for the selected model",
            ));
        }
        Ok(())
    }

    /// Decodes adapter-owned provider configuration without interpreting it in
    /// shared crates.
    pub fn from_provider_configuration(
        value: &ProviderConfiguration,
    ) -> Result<Self, GlmAdapterError> {
        if value.provider_id != provider_id()
            || value.values.namespace.as_str() != "provider.zai"
            || value.values.version != SchemaVersion::new(1).expect("static schema")
        {
            return Err(GlmAdapterError::Configuration(
                "provider envelope identity or version is unsupported",
            ));
        }
        let mut config = Self::default();
        let fields = &value.values.values;
        if let Some(model) = fields.get("zai:model").and_then(serde_json::Value::as_str) {
            config.model = ModelId::new(model)
                .map_err(|_| GlmAdapterError::Configuration("model ID is invalid"))?;
        }
        let plan = fields
            .get("zai:endpoint-plan")
            .and_then(serde_json::Value::as_str)
            .map(parse_plan)
            .transpose()?
            .unwrap_or(GlmPlan::Coding);
        config.endpoint = if plan == GlmPlan::Custom {
            let url = fields
                .get("zai:base-url")
                .and_then(serde_json::Value::as_str)
                .ok_or(GlmAdapterError::Configuration(
                    "custom endpoint requires zai:base-url",
                ))?;
            let insecure = fields
                .get("zai:allow-insecure-http")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let attach = fields
                .get("zai:attach-inference-auth")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            GlmEndpoint::custom(url, insecure, attach)?
        } else {
            GlmEndpoint::official(plan)?
        };
        if let Some(mode) = fields
            .get("zai:reasoning-mode")
            .and_then(serde_json::Value::as_str)
        {
            config.reasoning = parse_reasoning(mode)?;
        }
        if let Some(profile) = fields
            .get("zai:generation-profile")
            .and_then(serde_json::Value::as_str)
        {
            config.generation_profile = parse_generation_profile(profile)?;
        }
        if let Some(model) = fields
            .get("zai:auxiliary-model")
            .and_then(serde_json::Value::as_str)
            .filter(|model| *model != "main")
        {
            config.auxiliary_model =
                Some(ModelId::new(model).map_err(|_| {
                    GlmAdapterError::Configuration("auxiliary model ID is invalid")
                })?);
        }
        if let Some(limit) = fields
            .get("zai:continuation-limit")
            .and_then(serde_json::Value::as_u64)
        {
            config.continuation_limit = u32::try_from(limit).map_err(|_| {
                GlmAdapterError::Configuration("continuation limit is out of range")
            })?;
        }
        config.validate()?;
        Ok(config)
    }
}

pub(crate) fn parse_plan(value: &str) -> Result<GlmPlan, GlmAdapterError> {
    match value {
        "coding" => Ok(GlmPlan::Coding),
        "standard" => Ok(GlmPlan::Standard),
        "bigmodel" => Ok(GlmPlan::BigModel),
        "custom" => Ok(GlmPlan::Custom),
        _ => Err(GlmAdapterError::Configuration("endpoint plan is unknown")),
    }
}

pub(crate) fn parse_reasoning(value: &str) -> Result<GlmReasoningMode, GlmAdapterError> {
    match value {
        "disabled" => Ok(GlmReasoningMode::Disabled),
        "enabled" => Ok(GlmReasoningMode::Enabled),
        "high" => Ok(GlmReasoningMode::High),
        "max" => Ok(GlmReasoningMode::Max),
        _ => Err(GlmAdapterError::Configuration("reasoning mode is unknown")),
    }
}

pub(crate) fn parse_generation_profile(
    value: &str,
) -> Result<GlmGenerationProfile, GlmAdapterError> {
    match value {
        "balanced" => Ok(GlmGenerationProfile::Balanced),
        "precise" => Ok(GlmGenerationProfile::Precise),
        "exploratory" => Ok(GlmGenerationProfile::Exploratory),
        _ => Err(GlmAdapterError::Configuration(
            "generation profile is unknown",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_endpoints_are_exact_and_suffix_lookalikes_are_not_trusted() {
        let coding = GlmEndpoint::official(GlmPlan::Coding).unwrap();
        assert_eq!(coding.trust(), EndpointTrust::Official);
        assert!(coding.attach_inference_auth());
        assert!(coding.quota_supported());
        assert_eq!(
            coding.chat_completions_url().unwrap().as_str(),
            "https://api.z.ai/api/coding/paas/v4/chat/completions"
        );

        let lookalike =
            GlmEndpoint::custom("https://api.z.ai.attacker.invalid/v4", false, false).unwrap();
        assert_eq!(lookalike.trust(), EndpointTrust::ConfiguredRemote);
        assert!(!lookalike.attach_inference_auth());
        assert!(!lookalike.quota_supported());
    }

    #[test]
    fn custom_http_requires_opt_in_and_never_inherits_quota_authority() {
        assert!(GlmEndpoint::custom("http://127.0.0.1:1234/v4", false, true).is_err());
        let local = GlmEndpoint::custom("http://127.0.0.1:1234/v4", true, false).unwrap();
        assert_eq!(local.trust(), EndpointTrust::Local);
        assert!(!local.attach_inference_auth());
        assert!(!local.quota_supported());
        assert!(local.quota_url().is_err());
    }

    #[test]
    fn model_plan_and_reasoning_constraints_fail_before_dispatch() {
        let mut config = GlmConfig {
            model: ModelId::new("glm-5-turbo").unwrap(),
            reasoning: GlmReasoningMode::Max,
            ..GlmConfig::default()
        };
        assert!(config.validate().is_err());

        config.model = ModelId::new("glm-5v-turbo").unwrap();
        config.reasoning = GlmReasoningMode::Enabled;
        config.endpoint = GlmEndpoint::official(GlmPlan::Coding).unwrap();
        assert_eq!(
            config.validate().unwrap_err(),
            GlmAdapterError::ModelPlanMismatch
        );
    }

    #[test]
    fn deep_reasoning_is_valid_on_both_flagship_models() {
        // High/max gate on the whole flagship line, not one model id.
        for model in ["glm-5.3", "glm-5.2"] {
            let config = GlmConfig {
                model: ModelId::new(model).unwrap(),
                reasoning: GlmReasoningMode::Max,
                ..GlmConfig::default()
            };
            assert!(config.validate().is_ok(), "{model} must accept max");
        }
    }

    #[test]
    fn default_model_is_the_current_flagship() {
        assert_eq!(GlmConfig::default().model.as_str(), "glm-5.3");
    }

    #[test]
    fn provider_configuration_selects_a_validated_auxiliary_model() {
        let mut configuration = ProviderConfiguration {
            provider_id: crate::provider_id(),
            values: vesper_domain::VersionedExtensionEnvelope {
                namespace: vesper_domain::ExtensionNamespace::new("provider.zai").unwrap(),
                version: vesper_domain::SchemaVersion::new(1).unwrap(),
                values: vesper_domain::ExtensionMap::default(),
            },
        };
        configuration
            .values
            .values
            .insert("zai:auxiliary-model", serde_json::json!("glm-5.2"))
            .unwrap();
        let decoded = GlmConfig::from_provider_configuration(&configuration).unwrap();
        assert_eq!(
            decoded.auxiliary_model.as_ref().map(ModelId::as_str),
            Some("glm-5.2")
        );

        configuration
            .values
            .values
            .insert("zai:auxiliary-model", serde_json::json!("invented"))
            .unwrap();
        assert_eq!(
            GlmConfig::from_provider_configuration(&configuration).unwrap_err(),
            GlmAdapterError::UnknownModel
        );
    }
}
