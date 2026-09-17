//! Red-first regression and workspace probe: enrollment must accept real
//! PRDs whose paragraph count (PRD + the AGENTS.md instruction chain)
//! exceeds the old 256 cap. VB-PRD-001 measures 284 (234 PRD + 16
//! docs/AGENTS.md + 34 root AGENTS.md), so the ceiling is raised to 512
//! with honest error text. Sources remain byte-capped at 256 KiB, which is
//! the real input bound.

use super::*;

struct NoopReviewer;
impl AcceptanceReviewer for NoopReviewer {
    fn inspect<'a>(
        &'a self,
        _root: &'a std::path::Path,
        _request: String,
        _cancel: std::sync::Arc<dyn vesper_agent::CancellationSignal>,
    ) -> vesper_agent::ToolFuture<'a, Result<String, String>> {
        Box::pin(async { Err("not used by these bounds tests".into()) })
    }
}

fn write_many_paragraph_prd(root: &std::path::Path, paragraphs: usize) {
    let body = (0..paragraphs)
        .map(|i| format!("Paragraph {i} of the distributed requirement set."))
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(root.join("PRD.md"), body).unwrap();
}

#[test]
fn enrollment_accepts_prd_above_the_old_paragraph_ceiling() {
    let root = tempfile::tempdir().unwrap();
    // 300 PRD paragraphs alone already exceeded the previous 256 cap, and
    // the real VB-PRD-001 case is 284 (PRD + AGENTS.md chain).
    write_many_paragraph_prd(root.path(), 300);
    let session =
        AcceptanceSession::open(root.path(), "PRD.md", std::sync::Arc::new(NoopReviewer)).unwrap();
    assert_eq!(session.status().objective, "PRD acceptance");
}

#[test]
fn enrollment_still_rejects_paragraph_counts_beyond_the_new_ceiling() {
    let root = tempfile::tempdir().unwrap();
    write_many_paragraph_prd(root.path(), 600);
    let error =
        match AcceptanceSession::open(root.path(), "PRD.md", std::sync::Arc::new(NoopReviewer)) {
            Ok(_) => panic!("600 paragraphs must stay out of bounds"),
            Err(error) => error,
        };
    assert!(
        error.contains("1–512"),
        "error must state the current ceiling honestly: {error}"
    );
}

/// Local verification that the REAL workspace VB-PRD-001 opens under the
/// repaired ceiling. Read-only against this checkout; ignored by default so
/// foreign CI layouts do not depend on repository documentation files.
#[test]
#[ignore = "workspace-layout probe; run explicitly with --ignored"]
fn real_vesper_bridge_prd_opens_under_the_raised_ceiling() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let repo = std::path::Path::new(&manifest)
        .ancestors()
        .nth(2)
        .expect("crate is at <repo>/crates/vesper-harness")
        .to_path_buf();
    let prd = "docs/Vesper bridge/Vesper_Bridge_PRD.md";
    assert!(repo.join(prd).is_file(), "VB-PRD-001 missing at {prd}");
    let session = match AcceptanceSession::open(&repo, prd, std::sync::Arc::new(NoopReviewer)) {
        Ok(session) => session,
        Err(error) => panic!("real PRD must enroll under the raised ceiling: {error}"),
    };
    let status = session.status();
    assert_eq!(status.objective, "PRD acceptance");
    assert_eq!(status.total_scenarios, 0);
    assert!(!status.is_verified());
}
