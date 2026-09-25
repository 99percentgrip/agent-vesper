//! Safe host projection for provider-owned output annotations.

use std::collections::BTreeSet;
use vesper_domain::ContentPart;

/// Renders typed provider citations while keeping all other opaque provider
/// state hidden. The adapter already bounds its wire data; this boundary
/// validates the small display subset again before either host renders it.
#[must_use]
pub fn render_provider_citations(parts: &[ContentPart]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut rendered = Vec::new();
    for part in parts {
        let ContentPart::ProviderOpaque(opaque) = part else {
            continue;
        };
        if opaque.kind != "citation" {
            continue;
        }
        let value = opaque.data.expose();
        let Some(url) = value.get("url").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if url.len() > 8192
            || !matches!(url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")), Some(rest) if !rest.is_empty())
            || url
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
            || !seen.insert(url.to_owned())
        {
            continue;
        }
        let title = value
            .get("title")
            .and_then(serde_json::Value::as_str)
            .filter(|title| {
                !title.is_empty() && title.len() <= 4096 && !title.chars().any(char::is_control)
            });
        rendered.push(match title {
            Some(title) => format!("{title}: {url}"),
            None => url.to_owned(),
        });
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_domain::{OpaqueContent, OpaqueProviderData, ProviderId};

    fn opaque(kind: &str, value: serde_json::Value) -> ContentPart {
        ContentPart::ProviderOpaque(OpaqueContent {
            provider_id: ProviderId::new("fixture").unwrap(),
            kind: kind.to_owned(),
            data: OpaqueProviderData::new(value).unwrap(),
        })
    }

    #[test]
    fn citations_render_once_and_other_opaque_state_stays_hidden() {
        let parts = vec![
            opaque(
                "citation",
                serde_json::json!({"title":"Source", "url":"https://example.test/a"}),
            ),
            opaque(
                "citation",
                serde_json::json!({"url":"https://example.test/a"}),
            ),
            opaque(
                "reasoning.encrypted_content",
                serde_json::json!({"secret":"never-render"}),
            ),
            opaque("citation", serde_json::json!({"url":"javascript:alert(1)"})),
        ];
        assert_eq!(
            render_provider_citations(&parts),
            ["Source: https://example.test/a"]
        );
    }
}
