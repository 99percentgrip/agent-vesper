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

/// Read a text file with CRLF/CR normalized to LF.
///
/// The corpus is committed LF (`fixtures/AGENTS.md` contract enforced by
/// `.gitattributes`), but a Windows checkout with autocrlf rewrites
/// working-tree files to CRLF before any test sees them. Normalizing at
/// read keeps the byte-identical comparison about the *pipeline*, not the
/// checkout platform. (The parser independently normalizes CRLF→LF per the
/// HTML5 spec; this guard covers the golden side of the comparison.)
fn read_lf(path: &std::path::Path) -> String {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{}: unreadable: {error}", path.display()));
    raw.replace("\r\n", "\n").replace('\r', "\n")
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
        let html = read_lf(&path);
        let output = vesper_web::pipeline::run_default_pipeline(&html);

        let full_golden = read_lf(&goldens.join(format!("{name}.full.md")));
        let fit_golden = read_lf(&goldens.join(format!("{name}.fit.md")));

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
        let html = read_lf(&path);
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
        let html = read_lf(&path);
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

/// VRO-14 PR-6 (release-blocker regression): a CRLF-translated copy of any
/// fixture must render byte-identically to its LF original — the HTML5
/// CRLF→LF normalization in `dom::parse` plus this suite's read-time
/// normalization are what make the corpus platform-stable. This test
/// simulates a Windows autocrlf checkout in-process.
#[test]
fn crlf_translated_input_renders_identically() {
    let root = corpus_root();
    for path in fixture_paths(&root) {
        let html = read_lf(&path);
        let crlf = html.replace('\n', "\r\n");
        let lf_out = vesper_web::pipeline::run_default_pipeline(&html);
        let crlf_out = vesper_web::pipeline::run_default_pipeline(&crlf);
        assert_eq!(
            lf_out.full_markdown,
            crlf_out.full_markdown,
            "{}: CRLF input must not change output",
            path.display()
        );
        assert_eq!(lf_out.fit_markdown, crlf_out.fit_markdown);
    }
}
