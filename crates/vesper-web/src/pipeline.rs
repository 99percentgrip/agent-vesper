//! The end-to-end perception pipeline: Parse → Strip → Prune → Convert.
//!
//! Pure composition of the stage modules. No I/O; every entry point maps
//! fetched HTML bytes to bounded Markdown plus a per-stage byte report.

use crate::arena::{self, Dom};
use crate::convert::{self, ConvertOptions};
use crate::density::TextDensityFilter;
use crate::dom::{self, document_text_bytes};
use crate::strip;

/// Per-stage byte accounting for one pipeline run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DensityReport {
    /// Raw HTML bytes handed to the pipeline.
    pub html_bytes: usize,
    /// Text bytes surviving the strip stage.
    pub strip_bytes: usize,
    /// Full-path Markdown bytes.
    pub full_markdown_bytes: usize,
    /// Fit-path Markdown bytes (after density pruning).
    pub fit_markdown_bytes: usize,
}

impl DensityReport {
    /// fit/full markdown ratio, 1.0 when full is empty.
    pub fn fit_over_full(&self) -> f32 {
        if self.full_markdown_bytes == 0 {
            1.0
        } else {
            self.fit_markdown_bytes as f32 / self.full_markdown_bytes as f32
        }
    }

    /// fit/raw ratio, 1.0 when html is empty.
    pub fn fit_over_raw(&self) -> f32 {
        if self.html_bytes == 0 {
            1.0
        } else {
            self.fit_markdown_bytes as f32 / self.html_bytes as f32
        }
    }
}

/// Output of one pipeline run.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineOutput {
    /// Markdown from the stripped tree (no density pruning).
    pub full_markdown: String,
    /// Markdown from the density-pruned surviving blocks.
    pub fit_markdown: String,
    /// Per-stage byte accounting.
    pub report: DensityReport,
}

/// Run the full perception pipeline over fetched HTML bytes.
///
/// Pure: no I/O, no clock, no network. Output truncation to the caller's
/// budget is a composition-boundary concern (PRD §1.7); this function
/// returns complete converted text.
pub fn run_pipeline(
    html: &str,
    filter: &TextDensityFilter,
    opts: &ConvertOptions,
) -> PipelineOutput {
    let html_bytes = html.len();

    // Full path: parse → strip → convert.
    let mut doc = dom::parse(html);
    strip::strip(&mut doc);
    let full_markdown = convert::document_to_markdown(&doc, opts);
    let strip_bytes = document_text_bytes(&doc);

    // Fit path: re-prune the stripped tree through the arena, then convert
    // the surviving top-level body blocks.
    let mut arena = Dom::from_document(&doc);
    filter.filter(&mut arena);
    let blocks = arena::surviving_elements(&arena);
    let fit_markdown = convert::blocks_to_markdown(&blocks, opts);

    PipelineOutput {
        report: DensityReport {
            html_bytes,
            strip_bytes,
            full_markdown_bytes: full_markdown.len(),
            fit_markdown_bytes: fit_markdown.len(),
        },
        full_markdown,
        fit_markdown,
    }
}

/// Run the full pipeline with beta's default fixed-threshold filter and
/// default converter options.
pub fn run_default_pipeline(html: &str) -> PipelineOutput {
    run_pipeline(
        html,
        &TextDensityFilter::fixed_default(),
        &ConvertOptions::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_reports_both_paths_and_densities() {
        let html = "<html><body>\
            <div class=\"nav promo\">Menu Menu Menu Menu Menu Menu</div>\
            <article><h1>Title</h1><p>Real content survives the pipeline.</p></article>\
            </body></html>";
        let out = run_default_pipeline(html);
        assert!(
            out.fit_markdown.contains("Real content survives"),
            "fit: {}",
            out.fit_markdown
        );
        assert!(out.report.html_bytes > 0);
        assert!(out.report.fit_over_full() <= 1.0);
    }

    #[test]
    fn report_ratios_guard_against_zero_division() {
        let out = run_default_pipeline("");
        assert_eq!(out.report.fit_over_full(), 1.0);
        assert_eq!(out.report.fit_over_raw(), 1.0);
        assert_eq!(out.report.html_bytes, 0);
    }
}
