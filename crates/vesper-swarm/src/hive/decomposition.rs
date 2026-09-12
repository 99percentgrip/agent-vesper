//! Strict, bounded navigator output. No scripted count-only production fallback.
use super::orchestrator::HiveError;
use serde::Deserialize;

pub(super) const INSTRUCTIONS: &str = "Return only JSON: {\"tasks\":[{\"prompt\":\"concrete bounded task\",\"required_capabilities\":[],\"depends_on\":[]}]}. Use 1-64 tasks. depends_on contains zero-based indices of prerequisite tasks. Dependencies must be acyclic. Each prompt is at most 16384 bytes. Capabilities are requirements only, never permission grants.";

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PlannedTask {
    pub prompt: String,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    tasks: Vec<PlannedTask>,
}

pub(super) fn parse(output: &str) -> Result<Vec<(usize, PlannedTask)>, HiveError> {
    if output.len() > 1_048_576 {
        return Err(HiveError::Admission("decomposition byte limit"));
    }
    let plan: Plan = serde_json::from_str(output)
        .map_err(|_| HiveError::Admission("invalid decomposition JSON"))?;
    if !(1..=64).contains(&plan.tasks.len()) {
        return Err(HiveError::Admission("task count outside 1..=64"));
    }
    for (index, task) in plan.tasks.iter().enumerate() {
        if task.prompt.trim().is_empty()
            || task.prompt.len() > 16_384
            || task.required_capabilities.len() > 32
            || task
                .required_capabilities
                .iter()
                .any(|name| name.is_empty() || name.len() > 256)
            || task.depends_on.len() > 63
            || task
                .depends_on
                .iter()
                .any(|id| *id == index || *id >= plan.tasks.len())
        {
            return Err(HiveError::Admission(
                "invalid decomposed task bounds/dependency",
            ));
        }
        let distinct: std::collections::BTreeSet<_> = task.depends_on.iter().collect();
        if distinct.len() != task.depends_on.len() {
            return Err(HiveError::Admission("duplicate dependency"));
        }
    }
    let mut remaining: Vec<_> = plan.tasks.into_iter().enumerate().collect();
    let mut completed = std::collections::BTreeSet::new();
    let mut ordered = Vec::new();
    while !remaining.is_empty() {
        let Some(index) = remaining
            .iter()
            .position(|(_, task)| task.depends_on.iter().all(|id| completed.contains(id)))
        else {
            return Err(HiveError::Admission("cyclic decomposition"));
        };
        let (id, task) = remaining.remove(index);
        completed.insert(id);
        ordered.push((id, task));
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_scripted_counts_and_invalid_dependencies() {
        for text in [
            "tasks: 3",
            "{}",
            r#"{"tasks":[]}"#,
            r#"{"tasks":[{"prompt":"a","depends_on":[0]}]}"#,
            r#"{"tasks":[{"prompt":"a","depends_on":[1]},{"prompt":"b","depends_on":[0]}]}"#,
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }
    #[test]
    fn preserves_real_task_text_and_dependency_order() {
        let tasks = parse(r#"{"tasks":[{"prompt":"consume result","depends_on":[1]},{"prompt":"produce result"}]}"#).unwrap();
        assert_eq!(tasks[0].0, 1);
        assert_eq!(tasks[0].1.prompt, "produce result");
        assert_eq!(tasks[1].0, 0);
    }
}
