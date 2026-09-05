//! VRO-14 PR-1 integration test: byte-identical corpus verification.
//!
//! Every fixture under `fixtures/web-oracle/` must render through the
//! production pipeline to EXACTLY the committed golden bytes (both the
//! full-markdown and the fit-markdown path). This is the determinism
//! contract: any intentional converter change regenerates the goldens via
//! `cargo run -p vesper-web --example gen-goldens -- <repo-root>`;
//! an unintentional change fails here.

use std::path::PathBuf;

fn corpus_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/vesper-web → ../../fixtures/web-oracle
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/web-oracle")
        .canonicalize()
        .expect("fixture corpus present")
}

#[test]
fn corpus_exists_with_expected_fixture_count() {
    let root = corpus_root();
    let count = fixture_paths(&root).len();
    assert!(count >= 10, "expected >= 10 fixtures, found {count}");
}

#[test]
fn golden_corpus_renders_byte_identical_full_and_fit() {
    let root = corpus_root();
    let goldens = root.join("goldens");
    for path in fixture_paths(&root) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let html = std::fs::read_to_string(&path).expect("read fixture");
        let output = vesper_web::pipeline::run_default_pipeline(&html);

        let full_golden = std::fs::read_to_string(goldens.join(format!("{name}.full.md")))
            .unwrap_or_else(|error| panic!("{name}: missing full golden: {error}"));
        let fit_golden = std::fs::read_to_string(goldens.join(format!("{name}.fit.md")))
            .unwrap_or_else(|error| panic!("{name}: missing fit golden: {error}"));

        assert_eq!(
            output.full_markdown, full_golden,
            "{name}: full markdown drifted from golden"
        );
        assert_eq!(
            output.fit_markdown, fit_golden,
            "{name}: fit markdown drifted from golden"
        );
    }
}

#[test]
fn golden_pipeline_is_deterministic_across_runs() {
    let root = corpus_root();
    for path in fixture_paths(&root) {
        let html = std::fs::read_to_string(&path).expect("read fixture");
        let one = vesper_web::pipeline::run_default_pipeline(&html);
        let two = vesper_web::pipeline::run_default_pipeline(&html);
        assert_eq!(one.full_markdown, two.full_markdown);
        assert_eq!(one.fit_markdown, two.fit_markdown);
    }
}

#[test]
fn corpus_fit_never_exceeds_full() {
    let root = corpus_root();
    for path in fixture_paths(&root) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let html = std::fs::read_to_string(&path).expect("read fixture");
        let output = vesper_web::pipeline::run_default_pipeline(&html);
        assert!(
            output.fit_markdown.len() <= output.full_markdown.len(),
            "{name}: fit {} > full {}",
            output.fit_markdown.len(),
            output.full_markdown.len()
        );
    }
}

fn fixture_paths(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .expect("corpus readable")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "html")
        })
        .collect();
    paths.sort();
    paths
}
