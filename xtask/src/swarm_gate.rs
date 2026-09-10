//! Cargo-metadata enforcement of the default-off swarm dependency boundary.
use std::collections::{BTreeMap, BTreeSet};

fn closure(features: &BTreeMap<String, Vec<String>>, root: &str) -> BTreeSet<String> {
    let mut visited = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(name) = pending.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        if let Some(children) = features.get(&name) {
            pending.extend(children.iter().cloned());
        }
    }
    visited
}

pub(super) fn validate(package: &super::Package) -> Result<(), String> {
    let defaults = closure(&package.features, "default");
    if defaults
        .iter()
        .any(|feature| feature == "swarm" || feature.ends_with("/swarm"))
    {
        return Err(format!(
            "{} enables swarm through default features",
            package.name
        ));
    }
    for dependency in &package.dependencies {
        if dependency.name != "vesper-swarm" || dependency.kind.as_deref() == Some("dev") {
            continue;
        }
        let name = dependency.rename.as_deref().unwrap_or(&dependency.name);
        let activates = |feature: &str| {
            feature == name
                || feature == format!("dep:{name}")
                || feature.starts_with(&format!("{name}/"))
        };
        if !dependency.optional || defaults.iter().any(|feature| activates(feature)) {
            return Err(format!(
                "{} must keep vesper-swarm optional and default-off",
                package.name
            ));
        }
        if !closure(&package.features, "swarm")
            .iter()
            .any(|feature| activates(feature))
        {
            return Err(format!(
                "{} must activate vesper-swarm through its swarm feature",
                package.name
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn package(optional: bool, features: serde_json::Value) -> super::super::Package {
        serde_json::from_value(serde_json::json!({
            "id":"test", "name":"vesper-harness", "features": features,
            "dependencies":[{"name":"vesper-swarm", "req":"=0.21.4", "source":null,
                "path":"../vesper-swarm", "kind":null, "optional":optional, "rename":null}]
        }))
        .unwrap()
    }
    #[test]
    fn requires_optional_explicit_gate_and_transitively_default_off() {
        assert!(
            validate(&package(
                true,
                serde_json::json!({"swarm":["dep:vesper-swarm"]})
            ))
            .is_ok()
        );
        assert!(
            validate(&package(
                false,
                serde_json::json!({"swarm":["dep:vesper-swarm"]})
            ))
            .is_err()
        );
        assert!(
            validate(&package(
                true,
                serde_json::json!({"other":["dep:vesper-swarm"]})
            ))
            .is_err()
        );
        for target in [
            "swarm",
            "dep:vesper-swarm",
            "vesper-swarm/extra",
            "vesper-harness/swarm",
        ] {
            assert!(
                validate(&package(
                    true,
                    serde_json::json!({
                        "default":["indirect"], "indirect":[target], "swarm":["dep:vesper-swarm"]
                    })
                ))
                .is_err()
            );
        }
    }
    #[test]
    fn renamed_dependencies_cannot_bypass_the_gate() {
        let mut p = package(
            true,
            serde_json::json!({"default":["dep:coord"], "swarm":["dep:coord"]}),
        );
        p.dependencies[0].rename = Some("coord".into());
        assert!(validate(&p).is_err());
        p.features.remove("default");
        assert!(validate(&p).is_ok());
    }
}
