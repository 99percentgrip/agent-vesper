//! Filter DSL. Mirrors the operator surface mem0 documents on `search()`
//! (`mem0/memory/main.py:_process_metadata_filters`). v1 supports exact
//! match plus a structured subset of the operators; AND/OR/NOT composition
//! applies to `extras` (the JSON metadata bag).

use std::collections::BTreeMap;

use serde_json::Value;

/// Single-field operator. Mirrors the operator keys in
/// `Memory.search(filters=...)`.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldOp {
    /// `{key: value}` — equality.
    Eq(Value),
    Ne(Value),
    In(Vec<Value>),
    Nin(Vec<Value>),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    Contains(String),
    IContains(String),
    /// Wildcard — any non-null value.
    Wildcard,
}

/// Boolean composition of metadata filters applied to `extras`.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterDsl {
    /// Match a single `extras` field. The key MUST be a string.
    Field(String, FieldOp),
    And(Vec<FilterDsl>),
    Or(Vec<FilterDsl>),
    Not(Box<FilterDsl>),
}

impl FilterDsl {
    /// Evaluate against a memory's `extras` map. Mirrors the oracle's
    /// semantics for the operators v1 supports.
    #[must_use]
    pub fn matches(&self, extras: &BTreeMap<String, Value>) -> bool {
        match self {
            FilterDsl::Field(key, op) => field_matches(extras, key, op),
            FilterDsl::And(parts) => parts.iter().all(|p| p.matches(extras)),
            FilterDsl::Or(parts) => parts.iter().any(|p| p.matches(extras)),
            FilterDsl::Not(inner) => !inner.matches(extras),
        }
    }
}

fn field_matches(extras: &BTreeMap<String, Value>, key: &str, op: &FieldOp) -> bool {
    let value = extras.get(key);
    match op {
        FieldOp::Eq(target) => value == Some(target),
        FieldOp::Ne(target) => value != Some(target),
        FieldOp::In(set) => value.is_some_and(|v| set.contains(v)),
        FieldOp::Nin(set) => value.is_none_or(|v| !set.contains(v)),
        FieldOp::Gt(t) => value.is_some_and(|v| compare(v, t).is_some_and(|c| c.is_gt())),
        FieldOp::Gte(t) => value.is_some_and(|v| compare(v, t).is_some_and(|c| !c.is_lt())),
        FieldOp::Lt(t) => value.is_some_and(|v| compare(v, t).is_some_and(|c| c.is_lt())),
        FieldOp::Lte(t) => value.is_some_and(|v| compare(v, t).is_some_and(|c| !c.is_gt())),
        FieldOp::Contains(s) => value.is_some_and(|v| value_as_str(v).contains(s.as_str())),
        FieldOp::IContains(s) => {
            value.is_some_and(|v| value_as_str(v).to_lowercase().contains(&s.to_lowercase()))
        }
        FieldOp::Wildcard => value.is_some(),
    }
}

fn value_as_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Three-way comparison across numeric and string JSON values. Returns
/// `None` if the two values are not order-comparable (e.g. string vs
/// number, or non-scalar).
fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    let tonum = |v: &Value| v.as_f64();
    if let (Some(x), Some(y)) = (tonum(a), tonum(b)) {
        return x.partial_cmp(&y);
    }
    match (a, b) {
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn extras() -> BTreeMap<String, Value> {
        let mut m = BTreeMap::new();
        m.insert("score".to_string(), json!(42));
        m.insert("tag".to_string(), json!("rust"));
        m.insert("name".to_string(), json!("Agent Vesper"));
        m
    }

    #[test]
    fn eq_ne_in_nin() {
        let e = extras();
        assert!(FilterDsl::Field("score".into(), FieldOp::Eq(json!(42))).matches(&e));
        assert!(!FilterDsl::Field("score".into(), FieldOp::Ne(json!(42))).matches(&e));
        assert!(FilterDsl::Field("tag".into(), FieldOp::In(vec![json!("rust")])).matches(&e));
        assert!(FilterDsl::Field("tag".into(), FieldOp::Nin(vec![json!("go")])).matches(&e));
    }

    #[test]
    fn numeric_comparisons() {
        let e = extras();
        assert!(FilterDsl::Field("score".into(), FieldOp::Gt(json!(40))).matches(&e));
        assert!(FilterDsl::Field("score".into(), FieldOp::Lte(json!(42))).matches(&e));
        assert!(!FilterDsl::Field("score".into(), FieldOp::Lt(json!(10))).matches(&e));
    }

    #[test]
    fn contains_and_icontains() {
        let e = extras();
        assert!(FilterDsl::Field("name".into(), FieldOp::Contains("Vesper".into())).matches(&e));
        assert!(FilterDsl::Field("name".into(), FieldOp::IContains("vesper".into())).matches(&e));
        assert!(!FilterDsl::Field("name".into(), FieldOp::Contains("Python".into())).matches(&e));
    }

    #[test]
    fn wildcard_and_composition() {
        let e = extras();
        assert!(FilterDsl::Field("tag".into(), FieldOp::Wildcard).matches(&e));
        assert!(!FilterDsl::Field("missing".into(), FieldOp::Wildcard).matches(&e));
        let and = FilterDsl::And(vec![
            FilterDsl::Field("tag".into(), FieldOp::Eq(json!("rust"))),
            FilterDsl::Field("score".into(), FieldOp::Gt(json!(0))),
        ]);
        assert!(and.matches(&e));
        let not = FilterDsl::Not(Box::new(FilterDsl::Field(
            "tag".into(),
            FieldOp::Eq(json!("go")),
        )));
        assert!(not.matches(&e));
    }
}
