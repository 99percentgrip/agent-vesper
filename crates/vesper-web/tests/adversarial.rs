//! VRO-14 PR-6 adversarial-resilience suite: the pipeline must survive
//! hostile inputs without panicking, without unbounded memory growth, and
//! without losing recoverable host content. All fixtures are offline and
//! deterministic under `fixtures/web-oracle/adversarial/`.
//!
//! What "survive" means here, honestly:
//! - every stage completes (parse → strip → prune → convert),
//! - the output is deterministic across runs,
//! - recoverable content survives; noise (iframes, refresh URLs) does not,
//! - the pipeline is pure: it never follows redirects or fetches anything.
//!
//! The pipeline is NOT a truncator — output budgets are enforced by the
//! tool layer (PRD §1.7) — so these tests assert content survival and
//! stage completion, not byte caps.

use std::path::PathBuf;
use std::time::Instant;

fn adversarial_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/web-oracle/adversarial")
        .canonicalize()
        .expect("adversarial fixture directory present")
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(adversarial_root().join(name))
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn survives(name: &str) -> vesper_web::pipeline::PipelineOutput {
    let html = fixture(name);
    let start = Instant::now();
    let output = vesper_web::pipeline::run_default_pipeline(&html);
    let elapsed = start.elapsed();
    // Debug-profile bound only (release numbers are the perf gates);
    // generous enough for the 2.4 MB fixtures under a cold debug build.
    assert!(
        elapsed.as_millis() < 5_000,
        "{name}: debug-profile completion took {elapsed:?}"
    );
    let again = vesper_web::pipeline::run_default_pipeline(&html);
    assert_eq!(
        output.full_markdown, again.full_markdown,
        "{name}: non-deterministic full output"
    );
    assert_eq!(
        output.fit_markdown, again.fit_markdown,
        "{name}: non-deterministic fit output"
    );
    output
}

#[test]
fn hundred_k_node_dom_survives_all_stages() {
    // ~52k elements → >100k DOM nodes with 5,200-deep nesting: the parse
    // depth cap (browser-consistent) must flatten the over-deep tail
    // instead of overflowing the recursive walks.
    let output = survives("a01-100k-nodes.html");
    // The fixture's real content sits at the leaf of the deep chain; with
    // the cap flattening the tail, that text must still be reachable.
    let text = &output.full_markdown;
    assert!(
        text.contains("content"),
        "deep-chain content must remain reachable: {} B output",
        text.len()
    );
    // Density sanity on the giant tree: fit never exceeds full.
    assert!(output.fit_markdown.len() <= output.full_markdown.len());
}

#[test]
fn malformed_html_never_panics() {
    let output = survives("a02-malformed.html");
    assert!(
        output.full_markdown.contains("Mixed Case Tag"),
        "recoverable content must survive malformed markup"
    );
    // HTML5 semantics: an unterminated <style> (raw text) legitimately
    // swallows everything to EOF — identical to browser behavior. Graceful
    // handling means no panic and the pre-style content surviving intact,
    // not the impossible recovery of text inside the raw-text block.
    assert!(
        !output.full_markdown.contains("after broken style"),
        "raw-text consumption must match browser semantics"
    );
}

#[test]
fn cross_origin_iframe_noise_is_contained() {
    let output = survives("a03-iframe-noise.html");
    assert!(
        output.full_markdown.contains("Real content that matters"),
        "host content must survive iframe noise"
    );
    // Iframes are stripped before conversion; their origins never enter
    // the markdown surface (the fetch layer never touches them).
    assert_eq!(
        output.full_markdown.matches("evil-").count()
            + output.full_markdown.matches("tracker.invalid").count(),
        0,
        "iframe origins must not leak into markdown"
    );
}

#[test]
fn meta_refresh_chain_is_inert() {
    let output = survives("a04-meta-refresh.html");
    // All 12 refresh directives live in <head><meta>, which strip removes;
    // the pipeline is pure and never follows any redirect.
    assert!(
        output.full_markdown.contains("After refresh chain"),
        "body content must survive the refresh chain"
    );
    assert_eq!(
        output.full_markdown.matches("hop-").count(),
        0,
        "meta refresh URLs must not enter the markdown"
    );
}

#[test]
fn huge_attributes_are_bounded() {
    let output = survives("a05-huge-attributes.html");
    assert!(
        output
            .full_markdown
            .contains("styled paragraph with real content"),
        "content inside the huge-attribute element must survive: {} B",
        output.full_markdown.len()
    );
    assert!(
        output.full_markdown.contains("toplevel content paragraph"),
        "trailing top-level content must survive"
    );
    // The megabyte-scale attribute payloads never enter the markdown: the
    // converter emits text and links, not attribute values.
    assert!(
        output.full_markdown.len() < 8_192,
        "huge attribute values must not leak into markdown ({} B)",
        output.full_markdown.len()
    );
}

#[test]
fn near_empty_document_yields_clean_empty_output() {
    let output = survives("a06-empty-edge.html");
    assert!(
        output.full_markdown.trim().is_empty(),
        "empty document must yield empty markdown, got {:?}",
        output.full_markdown
    );
}

#[test]
fn binary_garbage_bytes_are_rejected_leniently() {
    // Not a file: inline bytes with invalid UTF-8 and control characters.
    let bytes: &[u8] = &[
        0x00, 0x01, 0x02, b'<', b'h', b't', b'm', b'l', b'>', 0xFF, 0xFE, 0xC3, 0x28, b'<', b'/',
        b'h', b't', b'm', b'l', b'>',
    ];
    let lossy = String::from_utf8_lossy(bytes);
    let output = vesper_web::pipeline::run_default_pipeline(&lossy);
    assert!(
        output.full_markdown.len() < 1_024,
        "garbage input must not explode output"
    );
}

// ---------------------------------------------------------- perf gates
//
// Release-profile bounds (PRD §5.4): CI-skipped, run locally with
// `cargo test -p vesper-web --test adversarial --release -- --ignored`.

#[cfg(test)]
mod perf {
    use super::*;
    use vesper_web::snapshot::tests_support::build_document;

    /// Prune + Convert over the 100k-node fixture: < 150 ms (release).
    #[test]
    #[ignore = "release-profile perf gate"]
    fn perf_prune_and_convert_hundred_k_nodes() {
        let html = fixture("a01-100k-nodes.html");
        // Warm once (page faults), then measure the cold-path steady state.
        let _ = vesper_web::pipeline::run_default_pipeline(&html);
        let start = Instant::now();
        let output = vesper_web::pipeline::run_default_pipeline(&html);
        let elapsed = start.elapsed();
        println!("perf: prune+convert 100k nodes: {elapsed:?}");
        assert!(
            elapsed.as_millis() < 150,
            "prune+convert took {elapsed:?} (bound 150 ms)"
        );
        let _ = output;
    }

    /// Serialize a 5,000-interactable snapshot: < 50 ms (release).
    #[test]
    #[ignore = "release-profile perf gate"]
    fn perf_serialize_five_k_interactables() {
        // 5k buttons, each with its own text node child.
        let styles: &[(&str, &str)] = &[("cursor", "pointer")];
        #[allow(clippy::type_complexity)]
        let rows: Vec<(usize, &str, &str, &[(&str, &str)], Vec<usize>)> = (0..5_000)
            .flat_map(|i| {
                vec![
                    (i * 2, "button", "box", styles, vec![i * 2 + 1]),
                    (i * 2 + 1, "#text", "label", &[][..], vec![]),
                ]
            })
            .collect();
        let doc = build_document(&rows);
        let mut cache = vesper_web::selector_map::SelectorMapCache::new("perf");
        let _ = vesper_web::selector_map::serialize_interactable_map(&doc, &mut cache);
        // Steady state.
        let mut cache2 = vesper_web::selector_map::SelectorMapCache::new("perf");
        let start = Instant::now();
        let text = vesper_web::selector_map::serialize_interactable_map(&doc, &mut cache2);
        let elapsed = start.elapsed();
        println!("perf: 5k-interactable serialize: {elapsed:?}");
        assert!(
            elapsed.as_millis() < 50,
            "5k-interactable serialization took {elapsed:?} (bound 50 ms)"
        );
        assert!(text.lines().count() >= 5_000);
    }
}
