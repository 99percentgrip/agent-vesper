use crate::OpenAiCatalog;
use vesper_domain::BoundedString;
use vesper_provider::{
    PlanChangeReaction, SuperpowerPolicy, SuperpowerSideEffect, SuperpowerValue,
};

/// Model-specific native control policy shared by host menus and validation.
#[derive(Clone, Copy)]
pub struct OpenAiSuperpowerPolicy {
    pub mode: crate::auth::AuthenticationMode,
}
impl Default for OpenAiSuperpowerPolicy {
    fn default() -> Self {
        Self {
            mode: crate::auth::AuthenticationMode::ChatGpt,
        }
    }
}
fn label(value: &SuperpowerValue) -> Option<&str> {
    match value {
        SuperpowerValue::Choice { value } => Some(value.as_str()),
        _ => None,
    }
}
impl SuperpowerPolicy for OpenAiSuperpowerPolicy {
    fn valid_choices(
        &self,
        alias: &str,
        advertised: &[SuperpowerValue],
        _plan: &str,
        model: &str,
    ) -> Vec<SuperpowerValue> {
        advertised
            .iter()
            .filter(|v| {
                alias != "thinking"
                    || label(v).is_some_and(|v| {
                        OpenAiCatalog::reasoning_levels_for(model, self.mode).contains(&v)
                    })
            })
            .cloned()
            .collect()
    }
    fn validate(
        &self,
        alias: &str,
        value: &SuperpowerValue,
        _plan: &str,
        model: &str,
    ) -> Result<(), String> {
        if alias == "thinking"
            && !label(value)
                .is_some_and(|v| OpenAiCatalog::reasoning_levels_for(model, self.mode).contains(&v))
        {
            return Err("Reasoning effort is not supported by the selected OpenAI model".into());
        }
        if alias == "model" && label(value).is_none_or(|v| OpenAiCatalog::find(v).is_none()) {
            return Err("Unknown OpenAI model".into());
        }
        Ok(())
    }
    fn on_change(
        &self,
        alias: &str,
        value: &SuperpowerValue,
        _plan: &str,
    ) -> Vec<SuperpowerSideEffect> {
        if alias == "model"
            && let Some(model) = label(value)
        {
            let invalid = ["max", "none"]
                .into_iter()
                .filter(|effort| {
                    !OpenAiCatalog::reasoning_levels_for(model, self.mode).contains(effort)
                })
                .map(choice)
                .collect::<Vec<_>>();
            if invalid.is_empty() {
                return Vec::new();
            }
            return vec![SuperpowerSideEffect {
                target_alias: "thinking".into(),
                new_value: choice("high"),
                apply_only_if_current_in: invalid,
            }];
        }
        Vec::new()
    }
    fn on_plan_change(&self, _: &str, _: &str, _: &str) -> PlanChangeReaction {
        PlanChangeReaction::none()
    }
    fn reasoning_mode(&self, value: &SuperpowerValue) -> Option<BoundedString<64>> {
        BoundedString::new(label(value)?).ok()
    }
}
fn choice(value: &str) -> SuperpowerValue {
    SuperpowerValue::Choice {
        value: BoundedString::new(value).expect("static"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thinking_is_model_specific_and_switch_repairs_max() {
        let p = OpenAiSuperpowerPolicy::default();
        let values = [choice("low"), choice("max")];
        assert_eq!(
            p.valid_choices("thinking", &values, "", "gpt-5.4"),
            [choice("low")]
        );
        assert_eq!(
            p.valid_choices("thinking", &values, "", "gpt-5.6-sol"),
            values
        );
        assert!(
            p.validate("thinking", &choice("max"), "", "gpt-5.5")
                .is_err()
        );
        assert_eq!(
            p.on_change("model", &choice("gpt-5.5"), "")[0].new_value,
            choice("high")
        );
        assert!(
            p.validate("thinking", &choice("ultra"), "", "gpt-6-astra")
                .is_err()
        );
    }
}
