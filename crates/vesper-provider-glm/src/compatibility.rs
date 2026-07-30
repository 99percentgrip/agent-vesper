use vesper_domain::{LegacyGlmSettings, ModelId};

use crate::{
    GlmAdapterError, GlmGenerationProfile, GlmPlan, GlmReasoningMode, catalog::is_known_model,
    config::parse_generation_profile, config::parse_plan, config::parse_reasoning,
};

/// Explicit read/write-free translation of legacy GLM-only session settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyGlmConfiguration {
    /// Exact legacy model identifier.
    pub model: ModelId,
    /// Whether the frozen catalog recognizes it.
    pub model_known: bool,
    /// Endpoint plan.
    pub plan: GlmPlan,
    /// Reasoning mode.
    pub reasoning: GlmReasoningMode,
    /// Sampling profile.
    pub generation_profile: GlmGenerationProfile,
    /// Exact auxiliary model (`main` means selected primary model).
    pub auxiliary_model: String,
}

/// Translates borrowed frozen compatibility fields without opening a session
/// path or changing the source record.
pub fn translate_legacy_settings(
    value: LegacyGlmSettings<'_>,
) -> Result<LegacyGlmConfiguration, GlmAdapterError> {
    let model = ModelId::new(value.model)
        .map_err(|_| GlmAdapterError::Configuration("legacy model ID is invalid"))?;
    if value.auxiliary_model.trim().is_empty() || value.auxiliary_model.len() > 256 {
        return Err(GlmAdapterError::Configuration(
            "legacy auxiliary model is invalid",
        ));
    }
    Ok(LegacyGlmConfiguration {
        model_known: is_known_model(model.as_str()),
        model,
        plan: parse_plan(value.api_endpoint)?,
        reasoning: parse_reasoning(value.thought_level)?,
        generation_profile: parse_generation_profile(value.generation_profile)?,
        auxiliary_model: value.auxiliary_model.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_legacy_models_are_preserved_but_annotated() {
        let translated = translate_legacy_settings(LegacyGlmSettings {
            model: "glm-future",
            thought_level: "enabled",
            api_endpoint: "standard",
            generation_profile: "balanced",
            auxiliary_model: "main",
        })
        .unwrap();
        assert_eq!(translated.model.as_str(), "glm-future");
        assert!(!translated.model_known);
    }
}
