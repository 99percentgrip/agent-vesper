use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use thiserror::Error;

/// A literal replacement approved by a scenario manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizationRule {
    pub literal: String,
    pub replacement: String,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum NormalizationError {
    #[error("normalization literal must not be empty")]
    EmptyLiteral,
    #[error("normalization literals must be unique")]
    DuplicateLiteral,
    #[error("normalization replacements must be unique")]
    ReplacementCollision,
}

/// Applies only explicitly supplied literal substitutions.
///
/// Structural array order is preserved; this function never sorts events or
/// removes fields.
pub fn normalize_json(
    value: &Value,
    rules: &[NormalizationRule],
) -> Result<Value, NormalizationError> {
    validate_rules(rules)?;
    let replacements: BTreeMap<&str, &str> = rules
        .iter()
        .map(|rule| (rule.literal.as_str(), rule.replacement.as_str()))
        .collect();
    Ok(normalize_value(value, &replacements))
}

fn validate_rules(rules: &[NormalizationRule]) -> Result<(), NormalizationError> {
    let mut literals = BTreeSet::new();
    let mut replacements = BTreeSet::new();
    for rule in rules {
        if rule.literal.is_empty() {
            return Err(NormalizationError::EmptyLiteral);
        }
        if !literals.insert(rule.literal.as_str()) {
            return Err(NormalizationError::DuplicateLiteral);
        }
        if !replacements.insert(rule.replacement.as_str()) {
            return Err(NormalizationError::ReplacementCollision);
        }
    }
    Ok(())
}

fn normalize_value(value: &Value, replacements: &BTreeMap<&str, &str>) -> Value {
    match value {
        Value::String(text) => Value::String(normalize_text(text, replacements)),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|item| normalize_value(item, replacements))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), normalize_value(value, replacements)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn normalize_text(text: &str, replacements: &BTreeMap<&str, &str>) -> String {
    replacements
        .iter()
        .fold(text.to_owned(), |current, (literal, replacement)| {
            current.replace(literal, replacement)
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn preserves_event_order() {
        let value = json!({"events": [{"sequence": 2}, {"sequence": 1}]});
        assert_eq!(normalize_json(&value, &[]).unwrap(), value);
    }

    #[test]
    fn rejects_colliding_tokens() {
        let rules = [
            NormalizationRule {
                literal: "one".into(),
                replacement: "$ID".into(),
            },
            NormalizationRule {
                literal: "two".into(),
                replacement: "$ID".into(),
            },
        ];
        assert_eq!(
            normalize_json(&Value::Null, &rules),
            Err(NormalizationError::ReplacementCollision)
        );
    }
}
