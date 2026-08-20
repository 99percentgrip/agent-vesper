//! GLM superpower policy (VRO provider-routing fix).
//!
//! Ports the GLM model/plan/reasoning logic that previously lived inline in
//! the TUI behind a provider-neutral [`SuperpowerPolicy`] impl, so the harness
//! never names a concrete provider. Behavior is preserved verbatim — this is a
//! *relocation* of the existing rules, not a redesign:
//!
//! - `/model` candidates are filtered by API-plan support (`supports_plan`).
//! - `/thinking` candidates are restricted to `disabled`/`enabled` unless the
//!   active model is in the flagship deep-reasoning line (`glm-5.3`/`glm-5.2`).
//! - Selecting a model outside that line cascades a `thinking` reset to
//!   `enabled` when the current thinking is `high`/`max`.
//! - `/reasoning` values map 1:1 to the runtime reasoning mode
//!   (`disabled`/`enabled`/`high`/`max`).

use vesper_domain::BoundedString;
use vesper_provider::{
    PlanChangeReaction, SuperpowerPolicy, SuperpowerSideEffect, SuperpowerValue,
};

use crate::{GlmCatalog, GlmPlan, catalog::supports_deep_reasoning};

/// The GLM provider's superpower policy. Stateless; safe to share.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlmSuperpowerPolicy;

/// The current GLM flagship model — the fallback when the active model is
/// unavailable on a new plan. Deep reasoning (`high`/`max`) is gated on the
/// flagship line via [`supports_deep_reasoning`] (`glm-5.3` and `glm-5.2`).
const FLAGSHIP_MODEL: &str = "glm-5.3";

/// Advertised `thinking` choices that are always available regardless of model.
const BASE_THINKING: &[&str] = &["disabled", "enabled"];

/// Advertised `thinking` choices that only deep-reasoning models support.
const EXTENDED_THINKING: &[&str] = &["high", "max"];

/// The accepted runtime reasoning-mode labels.
const REASONING_MODES: &[&str] = &["disabled", "enabled", "high", "max"];

impl GlmSuperpowerPolicy {
    /// Maps the session's endpoint-plan string to a frozen [`GlmPlan`]. Mirrors
    /// the legacy `glm_plan` mapping exactly.
    fn plan(value: &str) -> GlmPlan {
        match value {
            "standard" => GlmPlan::Standard,
            "bigmodel" => GlmPlan::BigModel,
            _ => GlmPlan::Coding,
        }
    }

    /// Extracts the choice label from a `Choice` superpower value.
    fn choice_label(value: &SuperpowerValue) -> Option<&str> {
        match value {
            SuperpowerValue::Choice { value } => Some(value.as_str()),
            _ => None,
        }
    }

    /// `thinking` choice value with the given label.
    fn thinking_choice(label: &str) -> SuperpowerValue {
        SuperpowerValue::Choice {
            value: BoundedString::new(label).expect("static thinking label is bounded"),
        }
    }
}

impl SuperpowerPolicy for GlmSuperpowerPolicy {
    fn valid_choices(
        &self,
        alias: &str,
        advertised: &[SuperpowerValue],
        active_plan: &str,
        active_model: &str,
    ) -> Vec<SuperpowerValue> {
        match alias {
            // `/model`: keep only models available on the active API plan.
            "model" => {
                let plan = Self::plan(active_plan);
                advertised
                    .iter()
                    .filter(|value| {
                        Self::choice_label(value)
                            .is_some_and(|model| GlmCatalog::supports_plan(model, plan))
                    })
                    .cloned()
                    .collect()
            }
            // `/thinking`: `disabled`/`enabled` always; `high`/`max` only on
            // the deep-reasoning flagship line.
            "thinking" => advertised
                .iter()
                .filter(|value| {
                    let Some(label) = Self::choice_label(value) else {
                        return false;
                    };
                    BASE_THINKING.contains(&label)
                        || (supports_deep_reasoning(active_model)
                            && EXTENDED_THINKING.contains(&label))
                })
                .cloned()
                .collect(),
            // `/plan` and `/generation` are structurally constrained by their
            // advertised scales; the policy passes them through unchanged.
            "plan" | "generation" => advertised.to_vec(),
            // `/auxiliary`: `main` always; catalog models only when they are
            // available on the active API plan and are not vision models
            // (vision models are not auxiliary-eligible).
            "auxiliary" => {
                let plan = Self::plan(active_plan);
                advertised
                    .iter()
                    .filter(|value| match Self::choice_label(value) {
                        Some("main") => true,
                        Some(model) => {
                            GlmCatalog::supports_plan(model, plan)
                                && !GlmCatalog::is_vision_model(model)
                        }
                        None => false,
                    })
                    .cloned()
                    .collect()
            }
            // Mixture-of-agents advisers (PRD FR-6/D5): the harness sends
            // tool-capable catalog candidates; GLM narrows to non-vision
            // models available on the active plan.
            "mixture" => {
                let plan = Self::plan(active_plan);
                advertised
                    .iter()
                    .filter(|value| match Self::choice_label(value) {
                        Some(model) => {
                            GlmCatalog::supports_plan(model, plan)
                                && !GlmCatalog::is_vision_model(model)
                        }
                        None => false,
                    })
                    .cloned()
                    .collect()
            }
            // Every other alias: no constraint.
            _ => advertised.to_vec(),
        }
    }

    fn validate(
        &self,
        alias: &str,
        value: &SuperpowerValue,
        active_plan: &str,
        _active_model: &str,
    ) -> Result<(), String> {
        if alias == "model"
            && let Some(model) = Self::choice_label(value)
            && !GlmCatalog::supports_plan(model, Self::plan(active_plan))
        {
            return Err(format!(
                "Model `{model}` is unavailable on the {active_plan} API plan."
            ));
        }
        // `/auxiliary` mirrors `valid_choices`: `main` always eligible;
        // catalog models only when plan-available and non-vision.
        if alias == "auxiliary"
            && let Some(model) = Self::choice_label(value)
            && model != "main"
        {
            let plan = Self::plan(active_plan);
            if !GlmCatalog::supports_plan(model, plan) {
                return Err(format!(
                    "Auxiliary model `{model}` is unavailable on the {active_plan} API plan."
                ));
            }
            if GlmCatalog::is_vision_model(model) {
                return Err(format!(
                    "Vision model `{model}` is not eligible for auxiliary work."
                ));
            }
        }
        Ok(())
    }

    fn on_change(
        &self,
        alias: &str,
        value: &SuperpowerValue,
        _active_plan: &str,
    ) -> Vec<SuperpowerSideEffect> {
        // Selecting a model outside the deep-reasoning flagship line cascades
        // a `thinking` reset to `enabled` — but only when the current thinking
        // is `high` or `max` (those are flagship-only modes the new model
        // cannot honor).
        if alias == "model"
            && Self::choice_label(value).is_some_and(|model| !supports_deep_reasoning(model))
        {
            vec![SuperpowerSideEffect {
                target_alias: "thinking".to_string(),
                new_value: Self::thinking_choice("enabled"),
                apply_only_if_current_in: EXTENDED_THINKING
                    .iter()
                    .map(|label| Self::thinking_choice(label))
                    .collect(),
            }]
        } else {
            Vec::new()
        }
    }

    fn on_plan_change(
        &self,
        new_plan: &str,
        current_model: &str,
        current_auxiliary: &str,
    ) -> PlanChangeReaction {
        let plan = Self::plan(new_plan);
        let mut reaction = PlanChangeReaction {
            owns_plans: true,
            ..Default::default()
        };
        // Reset the model if it doesn't support the new plan.
        if !GlmCatalog::supports_plan(current_model, plan) {
            reaction.reset_model_to = Some(FLAGSHIP_MODEL.to_string());
        }
        // Reset the auxiliary model if it doesn't support the new plan or is a
        // vision model (vision models are not auxiliary-eligible).
        if current_auxiliary != "main"
            && (!GlmCatalog::supports_plan(current_auxiliary, plan)
                || GlmCatalog::is_vision_model(current_auxiliary))
        {
            reaction.reset_auxiliary_to = Some("main".to_string());
        }
        reaction
    }

    fn reasoning_mode(&self, reasoning_value: &SuperpowerValue) -> Option<BoundedString<64>> {
        let SuperpowerValue::Choice { value } = reasoning_value else {
            return None;
        };
        REASONING_MODES
            .contains(&value.as_str())
            .then(|| BoundedString::new(value.as_str()).expect("reasoning label is bounded"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(label: &str) -> SuperpowerValue {
        SuperpowerValue::Choice {
            value: BoundedString::new(label).unwrap(),
        }
    }

    fn advertised_models() -> Vec<SuperpowerValue> {
        // A representative slice of the GLM catalog: the flagship line
        // glm-5.3/glm-5.2 (all plans), glm-4.5v (Standard, vision). Provenance
        // of the rule is in catalog.rs.
        vec![choice("glm-5.3"), choice("glm-5.2"), choice("glm-4.5v")]
    }

    fn advertised_thinking() -> Vec<SuperpowerValue> {
        vec![
            choice("disabled"),
            choice("enabled"),
            choice("high"),
            choice("max"),
        ]
    }

    #[test]
    fn model_choices_filter_by_active_plan() {
        let policy = GlmSuperpowerPolicy;
        // Coding plan: glm-4.5v is excluded (vision, not on Coding).
        let coding = policy.valid_choices("model", &advertised_models(), "coding", "");
        let coding_labels: Vec<&str> = coding
            .iter()
            .filter_map(|v| GlmSuperpowerPolicy::choice_label(v))
            .collect();
        assert!(coding_labels.contains(&"glm-5.2"));
        assert!(!coding_labels.contains(&"glm-4.5v"));

        // Standard plan: both are available.
        let standard = policy.valid_choices("model", &advertised_models(), "standard", "");
        let standard_labels: Vec<&str> = standard
            .iter()
            .filter_map(|v| GlmSuperpowerPolicy::choice_label(v))
            .collect();
        assert!(standard_labels.contains(&"glm-4.5v"));
    }

    #[test]
    fn thinking_choices_restrict_to_base_unless_flagship_line() {
        let policy = GlmSuperpowerPolicy;
        // Non-deep-reasoning active model: only disabled/enabled.
        let non_flag = policy.valid_choices("thinking", &advertised_thinking(), "", "glm-4.5v");
        let labels: Vec<&str> = non_flag
            .iter()
            .filter_map(|v| GlmSuperpowerPolicy::choice_label(v))
            .collect();
        assert_eq!(labels, vec!["disabled", "enabled"]);

        // The whole flagship line keeps the full range: glm-5.3 and glm-5.2.
        for flagship in ["glm-5.3", "glm-5.2"] {
            let full = policy.valid_choices("thinking", &advertised_thinking(), "", flagship);
            let labels: Vec<&str> = full
                .iter()
                .filter_map(|v| GlmSuperpowerPolicy::choice_label(v))
                .collect();
            assert!(
                labels.contains(&"high") && labels.contains(&"max"),
                "{flagship} must expose deep reasoning"
            );
        }
    }

    #[test]
    fn validate_rejects_model_unavailable_on_plan() {
        let policy = GlmSuperpowerPolicy;
        assert!(
            policy
                .validate("model", &choice("glm-4.5v"), "coding", "")
                .is_err()
        );
        assert!(
            policy
                .validate("model", &choice("glm-5.2"), "coding", "")
                .is_ok()
        );
        // Non-model aliases are always accepted.
        assert!(policy.validate("thinking", &choice("high"), "", "").is_ok());
    }

    #[test]
    fn on_change_cascades_thinking_reset_only_outside_the_flagship_line() {
        let policy = GlmSuperpowerPolicy;
        // Switching to glm-4.5v cascades a thinking reset (guarded by high/max).
        let effects = policy.on_change("model", &choice("glm-4.5v"), "");
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].target_alias, "thinking");
        assert!(
            effects[0]
                .apply_only_if_current_in
                .contains(&choice("high"))
        );
        assert!(effects[0].apply_only_if_current_in.contains(&choice("max")));

        // Switching within the flagship line never cascades.
        for flagship in ["glm-5.3", "glm-5.2"] {
            assert!(
                policy.on_change("model", &choice(flagship), "").is_empty(),
                "{flagship} must keep deep thinking"
            );
        }
    }

    #[test]
    fn reasoning_mode_maps_accepted_labels_and_rejects_others() {
        let policy = GlmSuperpowerPolicy;
        assert_eq!(
            policy.reasoning_mode(&choice("high")),
            Some(BoundedString::new("high").unwrap())
        );
        assert_eq!(
            policy.reasoning_mode(&choice("disabled")),
            Some(BoundedString::new("disabled").unwrap())
        );
        assert!(policy.reasoning_mode(&choice("low")).is_none());
    }

    #[test]
    fn unknown_alias_is_unconstrained() {
        let policy = GlmSuperpowerPolicy;
        let advertised = vec![choice("anything")];
        assert_eq!(
            policy.valid_choices("effort", &advertised, "", ""),
            advertised
        );
        assert!(
            policy
                .validate("effort", &choice("anything"), "", "")
                .is_ok()
        );
    }

    fn labels(values: &[SuperpowerValue]) -> Vec<&str> {
        values
            .iter()
            .filter_map(|v| GlmSuperpowerPolicy::choice_label(v))
            .collect()
    }

    #[test]
    fn auxiliary_choices_keep_main_and_filter_vision_and_plan() {
        let policy = GlmSuperpowerPolicy;
        let advertised = vec![choice("main"), choice("glm-5.3"), choice("glm-4.5v")];
        // Coding plan: glm-4.5v excluded (off-plan and vision).
        let coding = policy.valid_choices("auxiliary", &advertised, "coding", "");
        assert_eq!(labels(&coding), vec!["main", "glm-5.3"]);
        // Standard plan: glm-4.5v is plan-available but still vision → excluded.
        let standard = policy.valid_choices("auxiliary", &advertised, "standard", "");
        assert_eq!(labels(&standard), vec!["main", "glm-5.3"]);
    }

    #[test]
    fn auxiliary_validate_rejects_vision_and_off_plan_models() {
        let policy = GlmSuperpowerPolicy;
        assert!(
            policy
                .validate("auxiliary", &choice("main"), "coding", "")
                .is_ok()
        );
        assert!(
            policy
                .validate("auxiliary", &choice("glm-5.3"), "coding", "")
                .is_ok()
        );
        // Vision models are never auxiliary-eligible, whatever the plan.
        assert!(
            policy
                .validate("auxiliary", &choice("glm-4.5v"), "standard", "")
                .is_err()
        );
        assert!(
            policy
                .validate("auxiliary", &choice("glm-4.5v"), "coding", "")
                .is_err()
        );
    }

    #[test]
    fn mixture_choices_exclude_vision_and_off_plan_advisers() {
        let policy = GlmSuperpowerPolicy;
        let advertised = advertised_models();
        let coding = policy.valid_choices("mixture", &advertised, "coding", "");
        assert_eq!(labels(&coding), vec!["glm-5.3", "glm-5.2"]);
        // Vision models stay excluded from text-adviser duty on every plan.
        let standard = policy.valid_choices("mixture", &advertised, "standard", "");
        assert_eq!(labels(&standard), vec!["glm-5.3", "glm-5.2"]);
    }

    #[test]
    fn plan_and_generation_choices_pass_through_advertised_scales() {
        let policy = GlmSuperpowerPolicy;
        let plans = vec![choice("coding"), choice("standard"), choice("bigmodel")];
        assert_eq!(policy.valid_choices("plan", &plans.clone(), "", ""), plans);
        let styles = vec![choice("balanced"), choice("precise"), choice("exploratory")];
        assert_eq!(
            policy.valid_choices("generation", &styles.clone(), "", ""),
            styles
        );
    }
}
