use std::collections::{BTreeMap, BTreeSet};
use vesper_memory::{SkillRoutingQuery, SkillStore};

#[test]
fn literal_invocations_do_not_become_explicit_requests() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    let tools = BTreeSet::new();
    let outcomes = BTreeMap::new();
    for prompt in [
        "Explain `use skill missing`.",
        "The log says \"use skill missing\".",
        "Translate ‘use skill missing’ into French.",
        "Example:\n```text\nuse skill missing\n```",
        "Example:\n~~~text\nuse bundle missing\n~~~",
        "> use skill missing\nExplain that quotation.",
        "Do not use skill missing.",
        "Don't use skill missing.",
    ] {
        let result = store.orchestrate(&SkillRoutingQuery {
            prompt,
            explicit_skill: None,
            available_tools: &tools,
            platform: "linux",
            outcome_adjustments: &outcomes,
        });
        assert!(
            result.explicit_error.is_none(),
            "{prompt}: {:?}",
            result.explicit_error
        );
    }
}
