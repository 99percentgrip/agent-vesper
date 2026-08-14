//! HTML injector for the VesperLens review overlay (ADR 0017).
//!
//! [`inject_review_overlay`] is a pure function that takes the agent's raw
//! HTML artifact and returns a new HTML string with the VesperLens review
//! overlay `<script>` and `<style>` inserted just before `</body>` (or
//! appended at the end if no `</body>` is present).
//!
//! ## What this is NOT
//!
//! - It is **not** a port of lavish-axi's `chrome-client.js` (1878 lines)
//!   or `artifact-sdk.js` (1905 lines). Those modules were flagged by the
//!   harness content scanner and are not imported here.
//! - The overlay contains no code that the agent-generated HTML could
//!   influence at inject time. All review-panel strings are hard-coded
//!   literals owned by this crate. The only thing the overlay does with
//!   the page DOM is read it (to compute selectors on user click) and POST
//!   the resulting JSON contract to `/feedback`.
//!
//! ## Security
//!
//! The overlay trusts the served HTML exactly as a browser would — it
//! executes inside the same document. The *agent* only ever receives the
//! [`super::types::LensFeedback`] struct back; it never receives raw HTML.
//! User-supplied `comment` / `notes` strings are treated as untrusted input
//! by every downstream consumer.

/// The review-overlay JavaScript, as a single owned string.
///
/// Kept inline (rather than `include_str!`-loaded) so that:
/// (a) the overlay ships as a literal in the binary and is reviewable in
///     this source file, and
/// (b) there is no separate asset file the architecture gate would need to
///     know about.
///
/// The overlay:
/// 1. Injects a floating review panel (fixed top-right, ~280px wide).
/// 2. Provides Approve / Reject / Modify buttons.
/// 3. In Modify mode, attaches a click handler that captures the clicked
///    element's CSS selector and prompts for a comment, collecting
///    annotations into a list.
/// 4. On submit, POSTs `{action, annotations, notes}` as JSON to
///    `/feedback` and replaces the panel with a success message.
/// 5. Disables itself after submit (single-turn contract).
const OVERLAY_SCRIPT: &str = r##"(function(){
  "use strict";
  if (window.__vesperLensBooted) return;
  window.__vesperLensBooted = true;

  var annotations = [];
  var modifyMode = false;
  var submitted = false;

  var style = document.createElement("style");
  style.textContent = [
    "#vl-panel{position:fixed;top:12px;right:12px;width:300px;z-index:2147483647;",
    "background:#1e1e2e;color:#cdd6f4;border:1px solid #45475a;border-radius:8px;",
    "font:13px/1.4 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;",
    "padding:12px;box-shadow:0 8px 24px rgba(0,0,0,.35);}",
    "#vl-panel h2{margin:0 0 8px;font-size:14px;color:#cba6f7;}",
    "#vl-panel button{cursor:pointer;border:1px solid #45475a;background:#313244;",
    "color:#cdd6f4;padding:6px 10px;border-radius:4px;margin:2px;font:inherit;}",
    "#vl-panel button:hover{background:#45475a;}",
    "#vl-panel button.primary{background:#a6e3a1;color:#1e1e2e;border-color:#a6e3a1;}",
    "#vl-panel button.danger{background:#f38ba8;color:#1e1e2e;border-color:#f38ba8;}",
    "#vl-panel textarea{width:100%;box-sizing:border-box;background:#11111b;color:#cdd6f4;",
    "border:1px solid #45475a;border-radius:4px;padding:6px;font:inherit;margin-top:6px;}",
    "#vl-panel .vl-row{margin-top:8px;}",
    "#vl-panel .vl-note{font-size:11px;color:#9399b2;margin-top:6px;}",
    "#vl-annot-list{margin-top:6px;max-height:120px;overflow:auto;}",
    "#vl-annot-list .vl-item{background:#11111b;padding:4px 6px;border-radius:4px;margin-top:4px;",
    "font-size:11px;word-break:break-word;}",
    ".vl-highlight{outline:2px solid #f9e2af !important;outline-offset:1px;}"
  ].join("");
  document.head.appendChild(style);

  var panel = document.createElement("div");
  panel.id = "vl-panel";
  document.body.appendChild(panel);

  function render() {
    panel.innerHTML = "";
    var title = document.createElement("h2");
    title.textContent = "VesperLens Review";
    panel.appendChild(title);

    if (submitted) {
      var ok = document.createElement("div");
      ok.textContent = "Feedback sent \u2713  You may close this tab.";
      ok.className = "vl-note";
      ok.style.color = "#a6e3a1";
      panel.appendChild(ok);
      return;
    }

    var row = document.createElement("div");
    row.className = "vl-row";
    var approve = btn("Approve", "primary", function(){ submit("approve"); });
    var reject = btn("Reject", "danger", function(){ submit("reject"); });
    var modify = btn("Modify", "", toggleModify);
    row.appendChild(approve); row.appendChild(reject); row.appendChild(modify);
    panel.appendChild(row);

    var hint = document.createElement("div");
    hint.className = "vl-note";
    hint.textContent = modifyMode
      ? "Modify mode ON \u2014 click any element to annotate it."
      : "Click Modify to annotate specific elements.";
    panel.appendChild(hint);

    var list = document.createElement("div");
    list.id = "vl-annot-list";
    annotations.forEach(function(a, i){
      var item = document.createElement("div");
      item.className = "vl-item";
      item.textContent = "[" + (i+1) + "] " + a.selector + " \u2014 " + a.comment;
      list.appendChild(item);
    });
    panel.appendChild(list);

    var notesLabel = document.createElement("div");
    notesLabel.className = "vl-note";
    notesLabel.textContent = "Overall notes (optional):";
    panel.appendChild(notesLabel);
    var notes = document.createElement("textarea");
    notes.id = "vl-notes";
    notes.rows = 2;
    notes.placeholder = "Overall feedback for the agent...";
    panel.appendChild(notes);
  }

  function btn(label, cls, onClick) {
    var b = document.createElement("button");
    b.textContent = label;
    if (cls) b.className = cls;
    b.addEventListener("click", onClick);
    return b;
  }

  function toggleModify() {
    modifyMode = !modifyMode;
    if (modifyMode) {
      document.body.addEventListener("click", onBodyClick, true);
    } else {
      document.body.removeEventListener("click", onBodyClick, true);
    }
    render();
  }

  function onBodyClick(ev) {
    if (submitted) return;
    if (panel.contains(ev.target)) return; // ignore clicks on the panel itself
    ev.preventDefault();
    ev.stopPropagation();
    var el = ev.target;
    var selector = cssPath(el);
    var prev = el.getAttribute("data-vl") === "1";
    if (prev) {
      // second click on the same element removes its annotation
      annotations = annotations.filter(function(a){ return a.selector !== selector; });
      el.classList.remove("vl-highlight");
      el.removeAttribute("data-vl");
      render();
      return;
    }
    var comment = window.prompt("Comment for " + selector + ":", "");
    if (comment === null) return; // user cancelled
    el.classList.add("vl-highlight");
    el.setAttribute("data-vl", "1");
    annotations.push({ selector: selector, comment: comment || "", suggested_html: null });
    render();
  }

  function cssPath(el) {
    if (el.id) return "#" + el.id;
    var parts = [];
    while (el && el.nodeType === 1 && parts.length < 6) {
      var part = el.nodeName.toLowerCase();
      if (el.className && typeof el.className === "string") {
        var c = el.className.trim().split(/\s+/).slice(0, 2).join(".");
        if (c) part += "." + c;
      }
      var sib = el, nth = 1;
      while ((sib = sib.previousElementSibling)) nth++;
      part += ":nth-of-type(" + nth + ")";
      parts.unshift(part);
      el = el.parentElement;
    }
    return parts.join(" > ");
  }

  function submit(action) {
    if (submitted) return;
    var notesEl = document.getElementById("vl-notes");
    var notes = notesEl ? notesEl.value : "";
    var payload = {
      action: action,
      annotations: annotations,
      notes: notes
    };
    submitted = true;
    document.body.removeEventListener("click", onBodyClick, true);
    render();
    fetch("/feedback", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
      keepalive: true
    }).then(function(){
      render();
    }).catch(function(err){
      submitted = false;
      render();
      window.alert("VesperLens: failed to submit feedback: " + err);
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", render);
  } else {
    render();
  }
})();""##;

/// Inject the VesperLens review overlay into an HTML artifact.
///
/// - If the artifact contains a `</body>` (case-insensitive, allowing
///   trailing whitespace), the overlay is inserted immediately before it.
/// - Otherwise the overlay is appended to the end.
///
/// The function is idempotent in the sense that calling it on already-
/// injected HTML will inject the overlay a second time (this is harmless
/// because the overlay guards on `window.__vesperLensBooted`). Callers
/// should inject exactly once.
pub fn inject_review_overlay(html: &str) -> String {
    let overlay = build_overlay_tag();
    // Case-insensitive search for the LAST occurrence of </body>. We use
    // the last to handle pathological HTML that includes the literal
    // "</body>" inside a string earlier in the document. Case-insensitive
    // because real-world HTML is often `<BODY>` or `<Body>` (HTML is
    // case-insensitive for tag names per the HTML spec).
    if let Some(idx) = rfind_ci(html, "</body") {
        // Find the closing '>' of the </body...> tag.
        if html[idx..].find('>').is_some() {
            let mut out = String::with_capacity(html.len() + overlay.len());
            // Insert the overlay BEFORE the `</body...>` tag, then keep
            // the original `</body>` (and everything after) verbatim.
            out.push_str(&html[..idx]);
            out.push_str(&overlay);
            out.push_str(&html[idx..]);
            return out;
        }
    }
    // No </body> — append.
    let mut out = String::with_capacity(html.len() + overlay.len());
    out.push_str(html);
    out.push_str(&overlay);
    out
}

fn build_overlay_tag() -> String {
    format!("\n<!-- VesperLens review overlay (ADR 0017) -->\n<script>{OVERLAY_SCRIPT}</script>\n")
}

/// Case-insensitive `rfind`. Byte offsets in the lowercased haystack equal
/// those in the original because `to_ascii_lowercase` is byte-for-byte
/// (non-ASCII bytes are preserved unchanged).
fn rfind_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .rfind(&needle.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_before_body_close_tag() {
        let html = "<html><body><h1>hi</h1></body></html>";
        let out = inject_review_overlay(html);
        let body_pos = out.rfind("</body>").unwrap();
        let script_pos = out.find("<script>").unwrap();
        assert!(script_pos < body_pos, "script must precede </body>");
        assert!(out.contains("VesperLens Review"));
    }

    #[test]
    fn injects_before_body_close_tag_with_whitespace() {
        let html = "<html><body><p>x</p></body  \n  ></html>";
        let out = inject_review_overlay(html);
        assert!(out.contains("<script>"));
        // The </body ...> tag must come AFTER the script.
        let script_pos = out.find("<script>").unwrap();
        let body_close = out.find("</body").unwrap();
        assert!(script_pos < body_close);
    }

    #[test]
    fn appends_when_no_body_close_tag() {
        let html = "<html><h1>fragments only</h1>";
        let out = inject_review_overlay(html);
        assert!(out.ends_with("</script>\n"));
        assert!(out.contains("VesperLens Review"));
    }

    #[test]
    fn handles_empty_html() {
        let out = inject_review_overlay("");
        assert!(out.contains("<script>"));
    }

    #[test]
    fn injects_exactly_one_overlay_tag() {
        let html = "<html><body></body></html>";
        let out = inject_review_overlay(html);
        let count = out.matches("<script>").count();
        assert_eq!(count, 1, "exactly one overlay <script> should be injected");
    }

    #[test]
    fn preserves_original_html_bytes() {
        let html = "<html><body><p>preserve me</p></body></html>";
        let out = inject_review_overlay(html);
        assert!(out.contains("<p>preserve me</p>"));
        assert!(out.contains("</html>"));
    }

    #[test]
    fn overlay_script_does_not_reference_external_urls() {
        // The overlay must not load any external resource. fetch() to
        // "/feedback" is the only network call.
        assert!(!OVERLAY_SCRIPT.contains("http://"));
        assert!(!OVERLAY_SCRIPT.contains("https://"));
        assert!(!OVERLAY_SCRIPT.contains("src="));
        assert!(!OVERLAY_SCRIPT.contains("<link"));
    }

    #[test]
    fn overlay_script_targets_only_relative_feedback_path() {
        assert!(OVERLAY_SCRIPT.contains("fetch(\"/feedback\""));
    }

    #[test]
    fn case_insensitive_body_tag() {
        let html = "<html><BODY><p>x</p></BODY></html>";
        let out = inject_review_overlay(html);
        let script_pos = out.find("<script>").unwrap();
        let body_close = out.find("</BODY").unwrap();
        assert!(script_pos < body_close);
    }
}
