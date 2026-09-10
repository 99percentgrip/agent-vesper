//! Stable, counted file/content identities; line numbers are diagnostics only.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone)]
pub(super) struct Hit {
    pub path: String,
    pub line: usize,
    pub sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    version: u32,
    entries: Vec<Entry>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    sha256: String,
    count: usize,
}
type Counts = BTreeMap<(String, String), usize>;
fn parse(text: &str) -> Result<Counts, String> {
    let baseline: Baseline =
        serde_json::from_str(text).map_err(|error| format!("invalid naming baseline: {error}"))?;
    if baseline.version != 1 {
        return Err("unsupported naming baseline version".into());
    }
    let mut counts = Counts::new();
    for entry in baseline.entries {
        if entry.path.is_empty()
            || entry.path.contains(['\\', ':'])
            || entry
                .path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || entry.count == 0
            || counts
                .insert((entry.path, entry.sha256), entry.count)
                .is_some()
        {
            return Err("invalid or duplicate naming baseline entry".into());
        }
    }
    Ok(counts)
}
pub(super) fn encode(hits: &[Hit]) -> Result<String, String> {
    let mut counts = Counts::new();
    for hit in hits {
        *counts
            .entry((hit.path.clone(), hit.sha256.clone()))
            .or_default() += 1;
    }
    let entries = counts
        .into_iter()
        .map(|((path, sha256), count)| Entry {
            path,
            sha256,
            count,
        })
        .collect();
    let text = serde_json::to_string_pretty(&Baseline {
        version: 1,
        entries,
    })
    .map_err(|error| error.to_string())?;
    parse(&text)?;
    Ok(format!("{text}\n"))
}
pub(super) fn violations<'a>(text: &str, hits: &'a [Hit]) -> Result<Vec<&'a Hit>, String> {
    let mut allowed = parse(text)?;
    Ok(hits
        .iter()
        .filter(
            |hit| match allowed.get_mut(&(hit.path.clone(), hit.sha256.clone())) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    false
                }
                _ => true,
            },
        )
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hit(line: usize) -> Hit {
        Hit {
            path: "docs/example.md".into(),
            line,
            sha256: "a".repeat(64),
        }
    }
    #[test]
    fn line_shift_is_not_a_new_hit_but_duplicate_occurrences_are() {
        let frozen = encode(&[hit(10)]).unwrap();
        assert!(violations(&frozen, &[hit(20)]).unwrap().is_empty());
        assert_eq!(violations(&frozen, &[hit(20), hit(30)]).unwrap().len(), 1);
        assert!(violations(&frozen, &[]).unwrap().is_empty());
    }
    #[test]
    fn content_changes_and_path_moves_remain_new_hits() {
        let frozen = encode(&[hit(10)]).unwrap();
        let mut changed = hit(10);
        changed.sha256 = "b".repeat(64);
        let mut moved = hit(10);
        moved.path = "docs/other.md".into();
        assert_eq!(violations(&frozen, &[changed, moved]).unwrap().len(), 2);
    }
    #[test]
    fn malformed_duplicate_and_unknown_version_baselines_fail_closed() {
        let frozen = encode(&[hit(10)]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&frozen).unwrap();
        for bad in [
            serde_json::json!({"version":2,"entries":[]}),
            serde_json::json!({"version":1,"entries":[value["entries"][0].clone(),value["entries"][0].clone()]}),
            serde_json::json!({"version":1,"entries":[{"path":"../escape","sha256":"a".repeat(64),"count":1}]}),
            serde_json::json!({"version":1,"entries":[{"path":"file","sha256":"invalid","count":1}]}),
        ] {
            assert!(violations(&bad.to_string(), &[]).is_err());
        }
        assert!(violations("not json", &[]).is_err());
    }
    #[test]
    fn encoding_is_stable_and_preserves_occurrence_counts() {
        let mut second = hit(40);
        second.path = "README.md".into();
        let first = encode(&[hit(1), second.clone(), hit(2)]).unwrap();
        assert_eq!(first, encode(&[second, hit(999), hit(888)]).unwrap());
        assert!(violations(&first, &[hit(10), hit(20)]).unwrap().is_empty());
    }
}
