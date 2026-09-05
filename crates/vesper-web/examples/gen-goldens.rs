//! VRO-14 PR-1 golden-generation binary: renders the committed fixture
//! corpus (`fixtures/web-oracle/`) through the production pipeline and
//! writes the golden Markdown snapshots under `fixtures/web-oracle/goldens/`
//! (one `<fixture>.full.md` + `<fixture>.fit.md` per input).
//!
//! Determinism contract: two runs over the same crate version produce
//! byte-identical goldens; the integration test asserts every golden
//! byte-for-byte on every `cargo test` run. Run manually after any
//! intentional converter change:
//!
//! ```text
//! cargo run -p vesper-web --example gen-goldens -- <repo-root>
//! ```
//!
//! Not part of `cargo xtask verify`; it is a maintenance tool for the
//! golden corpus only.

use std::path::Path;

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let corpus = Path::new(&root).join("fixtures/web-oracle");
    let goldens = corpus.join("goldens");
    std::fs::create_dir_all(&goldens).expect("create goldens dir");

    let mut entries: Vec<_> = std::fs::read_dir(&corpus)
        .expect("read corpus dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "html")
        })
        .collect();
    entries.sort();

    assert!(
        entries.len() >= 10,
        "expected at least 10 fixtures, found {}",
        entries.len()
    );

    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let html = std::fs::read_to_string(&path).expect("read fixture");
        let output = vesper_web::pipeline::run_default_pipeline(&html);

        let full_path = goldens.join(format!("{name}.full.md"));
        let fit_path = goldens.join(format!("{name}.fit.md"));
        std::fs::write(&full_path, &output.full_markdown).expect("write full golden");
        std::fs::write(&fit_path, &output.fit_markdown).expect("write fit golden");
        println!(
            "{name}: full {} B, fit {} B",
            output.full_markdown.len(),
            output.fit_markdown.len()
        );
    }
    println!("goldens regenerated under {}", goldens.display());
}
