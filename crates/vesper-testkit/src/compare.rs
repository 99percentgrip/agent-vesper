use serde_json::Value;
use thiserror::Error;

use crate::fixture::ComparisonClass;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comparison {
    pub equal: bool,
    pub differences: Vec<String>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ComparisonError {
    #[error("semantic comparison requires objects")]
    SemanticRequiresObjects,
}

pub fn compare(
    class: ComparisonClass,
    expected: &Value,
    actual: &Value,
) -> Result<Comparison, ComparisonError> {
    match class {
        ComparisonClass::ExactOutput
        | ComparisonClass::SchemaCompatibility
        | ComparisonClass::SecurityInvariant => Ok(compare_exact(expected, actual)),
        ComparisonClass::SemanticParity | ComparisonClass::Performance => {
            compare_semantic(expected, actual)
        }
    }
}

fn compare_exact(expected: &Value, actual: &Value) -> Comparison {
    if expected == actual {
        Comparison {
            equal: true,
            differences: Vec::new(),
        }
    } else {
        Comparison {
            equal: false,
            differences: vec!["canonical JSON values differ".to_owned()],
        }
    }
}

/// Semantic comparison intentionally remains conservative: object keys present
/// in the expected value must match, and arrays remain order-sensitive.
fn compare_semantic(expected: &Value, actual: &Value) -> Result<Comparison, ComparisonError> {
    let (Value::Object(expected), Value::Object(actual)) = (expected, actual) else {
        return Err(ComparisonError::SemanticRequiresObjects);
    };
    let mut differences = Vec::new();
    for (key, expected_value) in expected {
        match actual.get(key) {
            Some(actual_value) if actual_value == expected_value => {}
            Some(_) => differences.push(format!("field {key:?} differs")),
            None => differences.push(format!("field {key:?} is missing")),
        }
    }
    Ok(Comparison {
        equal: differences.is_empty(),
        differences,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn semantic_comparison_allows_additional_fields() {
        let expected = json!({"events": [1, 2]});
        let actual = json!({"events": [1, 2], "diagnostic": "extra"});
        assert!(compare_semantic(&expected, &actual).unwrap().equal);
    }

    #[test]
    fn semantic_comparison_preserves_array_order() {
        let expected = json!({"events": [1, 2]});
        let actual = json!({"events": [2, 1]});
        assert!(!compare_semantic(&expected, &actual).unwrap().equal);
    }
}
