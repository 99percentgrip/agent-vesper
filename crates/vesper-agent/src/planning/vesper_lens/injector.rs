//! HTML injector for the VesperLens review overlay (ADR 0017).
//!
//! [`inject_review_overlay`] is a pure function that takes the agent's raw
//! HTML artifact and returns a new HTML string with the VesperLens review
//! overlay `<script>` and `<style>` inserted just before `</body>` (or
//! appended at the end if no `</body>` is present).
//!
//! ## What this is NOT
//!
//! - It is **not** a port of the reference Oracle's `chrome-client.js`
//!   (1878 lines) or `artifact-sdk.js` (1905 lines). Those modules were
//!   flagged by the harness content scanner and are not imported here.
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
/// 1. Injects a floating review panel (fixed top-right, ~320px wide).
/// 2. Provides Approve (submit `approve`) / Request changes (submit
///    `reject`) / Annotate page (pick mode) actions.
/// 3. In pick mode: hovering outlines the element under the cursor; a
///    click opens an INLINE popover editor anchored at the click (no
///    `window.prompt`) that captures the comment; selecting text first
///    quotes the selection inside the note; a second click on an already
///    annotated element removes its annotation; Esc exits pick mode.
/// 4. Annotations render as a removable numbered list (✕ per item).
/// 5. On submit, POSTs `{action, annotations, notes}` as JSON to
///    `/feedback` and replaces the panel with a success message.
/// 6. Disables itself after submit (single-turn contract).
const OVERLAY_SCRIPT: &str = r##"(function(){
  "use strict";
  if (window.__vesperLensBooted) return;
  window.__vesperLensBooted = true;

  // VRO-11.7 — Oracle-style review loop: pick mode with hover
  // highlight, an INLINE popover editor (never a native prompt dialog),
  // text-selection annotations, and a removable/editable annotation list.
  // All strings are owned literals; the only network call is the relative
  // POST /feedback.
  // Pick mode is ON BY DEFAULT (VRO-11.7): the page is immediately
  // interactive — hover outlines, click annotates — with no hidden mode
  // to discover. Esc or the panel button turns picking off.
  var annotations = [];
  var pickMode = true;
  var submitted = false;
  var popover = null;
  var popoverCtx = null; // {el, selector, quote}

  var style = document.createElement("style");
  style.textContent = [
    "#vl-panel{position:fixed;top:12px;right:12px;width:320px;z-index:2147483647;",
    "background:#1e1e2e;color:#cdd6f4;border:1px solid #45475a;border-radius:10px;",
    "font:13px/1.45 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;",
    "padding:14px;box-shadow:0 12px 32px rgba(0,0,0,.45);}",
    "#vl-panel h2{margin:0 0 10px;font-size:14px;color:#cba6f7;display:flex;",
    "align-items:center;gap:6px;}",
    "#vl-panel h2 .vl-dot{width:8px;height:8px;border-radius:50%;background:#89b4fa;",
    "display:inline-block;}",
    "#vl-panel button{cursor:pointer;border:1px solid #45475a;background:#313244;",
    "color:#cdd6f4;padding:6px 12px;border-radius:6px;margin:2px;font:inherit;}",
    "#vl-panel button:hover{background:#45475a;}",
    "#vl-panel button.primary{background:#a6e3a1;color:#1e1e2e;border-color:#a6e3a1;font-weight:600;}",
    "#vl-panel button.danger{background:#f38ba8;color:#1e1e2e;border-color:#f38ba8;font-weight:600;}",
    "#vl-panel button.on{background:#f9e2af;color:#1e1e2e;border-color:#f9e2af;font-weight:600;}",
    "#vl-panel textarea{width:100%;box-sizing:border-box;background:#11111b;color:#cdd6f4;",
    "border:1px solid #45475a;border-radius:6px;padding:7px;font:inherit;margin-top:8px;}",
    "#vl-panel .vl-row{margin-top:10px;display:flex;flex-wrap:wrap;}",
    "#vl-panel .vl-note{font-size:11px;color:#9399b2;margin-top:8px;}",
    "#vl-annot-list{margin-top:8px;max-height:150px;overflow:auto;}",
    "#vl-annot-list .vl-item{background:#11111b;padding:6px 8px;border-radius:6px;margin-top:5px;",
    "font-size:11.5px;word-break:break-word;border-left:3px solid #f9e2af;}",
    "#vl-annot-list .vl-item .vl-x{float:right;cursor:pointer;color:#f38ba8;font-weight:700;",
    "padding:0 3px;}",
    "#vl-annot-list .vl-item .vl-sel{color:#89b4fa;font-family:ui-monospace,monospace;",
    "font-size:10.5px;}",
    "#vl-popover{position:absolute;z-index:2147483647;background:#11111b;color:#cdd6f4;",
    "border:1px solid #89b4fa;border-radius:8px;padding:10px;width:260px;font:12.5px/1.4",
    " -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;box-shadow:0 8px 24px rgba(0,0,0,.5);}",
    "#vl-popover .vl-target{color:#89b4fa;font-family:ui-monospace,monospace;font-size:10.5px;",
    "word-break:break-all;margin-bottom:6px;}",
    "#vl-popover .vl-quote{color:#9399b2;font-style:italic;margin-bottom:6px;",
    "max-height:52px;overflow:hidden;}",
    "#vl-popover input{width:100%;box-sizing:border-box;background:#1e1e2e;color:#cdd6f4;",
    "border:1px solid #45475a;border-radius:6px;padding:6px;font:inherit;}",
    "#vl-popover .vl-row{margin-top:8px;}",
    ".vl-highlight{outline:2px solid #f9e2af !important;outline-offset:1px;}",
    ".vl-hover{outline:2px dashed #89b4fa !important;outline-offset:1px;}",
    "#vl-panel .vl-badge{display:inline-block;background:#313244;border-radius:10px;",
    "padding:1px 8px;font-size:10.5px;color:#9399b2;margin-left:6px;}"
  ].join("");
  document.head.appendChild(style);

  var panel = document.createElement("div");
  panel.id = "vl-panel";
  document.body.appendChild(panel);

  function esc(s){ var d = document.createElement("span"); d.textContent = s; return d; }

  function render() {
    panel.innerHTML = "";
    var title = document.createElement("h2");
    var dot = document.createElement("span"); dot.className = "vl-dot";
    title.appendChild(dot);
    title.appendChild(document.createTextNode("VesperLens Review"));
    if (annotations.length) {
      var badge = document.createElement("span");
      badge.className = "vl-badge";
      badge.textContent = annotations.length + " note" + (annotations.length > 1 ? "s" : "");
      title.appendChild(badge);
    }
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
    row.appendChild(btn("Approve", "primary", function(){ submit("approve"); }));
    row.appendChild(btn("Request changes", "danger", function(){ submit("reject"); }));
    var pick = btn(pickMode ? "Picking\u2026" : "Annotate page", pickMode ? "on" : "", togglePick);
    row.appendChild(pick);
    panel.appendChild(row);

    var hint = document.createElement("div");
    hint.className = "vl-note";
    hint.textContent = pickMode
      ? "Click any element to comment on it, or select text first to quote it. Esc stops picking."
      : "Annotate page \u2192 click elements / select text \u2192 leave inline notes for the agent.";
    panel.appendChild(hint);

    var list = document.createElement("div");
    list.id = "vl-annot-list";
    annotations.forEach(function(a, i){
      var item = document.createElement("div");
      item.className = "vl-item";
      var x = document.createElement("span");
      x.className = "vl-x";
      x.textContent = "\u2715";
      x.title = "Remove note";
      x.addEventListener("click", function(){ removeAnnotation(i); });
      item.appendChild(x);
      var sel = document.createElement("div");
      sel.className = "vl-sel";
      sel.textContent = (i + 1) + ". " + a.selector;
      item.appendChild(sel);
      if (a.comment) item.appendChild(esc(a.comment));
      else { var em = document.createElement("i"); em.textContent = "(no comment)"; item.appendChild(em); }
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

  function togglePick() { setPickMode(!pickMode); }

  function setPickMode(on) {
    pickMode = on;
    closePopover();
    if (on) {
      document.addEventListener("mouseover", onHover, true);
      document.addEventListener("click", onBodyClick, true);
      document.addEventListener("mouseup", onMouseUp, true);
      document.addEventListener("keydown", onKey, true);
    } else {
      document.removeEventListener("mouseover", onHover, true);
      document.removeEventListener("click", onBodyClick, true);
      document.removeEventListener("mouseup", onMouseUp, true);
      document.removeEventListener("keydown", onKey, true);
      clearHover();
    }
    render();
  }

  var hovered = null;
  function onHover(ev){
    if (submitted || pickMode === false) return;
    if (panel.contains(ev.target) || (popover && popover.contains(ev.target))) return;
    clearHover();
    hovered = ev.target;
    if (hovered.classList) hovered.classList.add("vl-hover");
  }
  function clearHover(){
    if (hovered && hovered.classList) hovered.classList.remove("vl-hover");
    hovered = null;
  }

  function onMouseUp(ev){
    if (submitted || !pickMode) return;
    if (panel.contains(ev.target) || (popover && popover.contains(ev.target))) return;
    var sel = window.getSelection();
    var text = sel ? String(sel) : "";
    if (!text.trim()) return;
    var node = sel.anchorNode;
    var el = node && node.nodeType === 1 ? node : (node ? node.parentElement : null);
    if (!el) return;
    ev.preventDefault(); ev.stopPropagation();
    clearHover();
    openPopover(ev, el, cssPath(el), text.trim().slice(0, 120));
  }

  function onBodyClick(ev) {
    if (submitted || !pickMode) return;
    if (panel.contains(ev.target) || (popover && popover.contains(ev.target))) return;
    ev.preventDefault();
    ev.stopPropagation();
    clearHover();
    var el = ev.target;
    var selector = cssPath(el);
    closePopover();
    var prev = el.getAttribute("data-vl") === "1";
    if (prev) {
      annotations = annotations.filter(function(a){ return a.selector !== selector; });
      el.classList.remove("vl-highlight");
      el.removeAttribute("data-vl");
      render();
      return;
    }
    openPopover(ev, el, selector, null);
  }

  function onKey(ev){
    if (ev.key === "Escape") {
      if (popover) { closePopover(); return; }
      setPickMode(false);
    }
  }

  function openPopover(ev, el, selector, quote) {
    closePopover();
    popoverCtx = { el: el, selector: selector, quote: quote };
    popover = document.createElement("div");
    popover.id = "vl-popover";
    var target = document.createElement("div");
    target.className = "vl-target";
    target.textContent = selector;
    popover.appendChild(target);
    if (quote) {
      var q = document.createElement("div");
      q.className = "vl-quote";
      q.textContent = "\u201C" + quote + "\u201D";
      popover.appendChild(q);
    }
    var input = document.createElement("input");
    input.type = "text";
    input.placeholder = "What should change here?";
    popover.appendChild(input);
    var row = document.createElement("div");
    row.className = "vl-row";
    var add = btn("Add note", "primary", confirmPopover);
    var cancel = btn("Cancel", "", closePopover);
    row.appendChild(add); row.appendChild(cancel);
    popover.appendChild(row);
    document.body.appendChild(popover);
    // Anchor near the click, clamped to the viewport.
    var x = Math.min((ev.clientX || 0) + 12, window.innerWidth - 280);
    var y = Math.min((ev.clientY || 0) + 12, window.innerHeight - 160);
    popover.style.left = Math.max(8, x) + "px";
    popover.style.top = Math.max(8, y) + "px";
    input.focus();
    input.addEventListener("keydown", function(e){
      if (e.key === "Enter") { e.preventDefault(); confirmPopover(); }
    });
  }

  function confirmPopover() {
    if (!popover || !popoverCtx) return;
    var input = popover.querySelector("input");
    var comment = input ? input.value.trim() : "";
    if (popoverCtx.quote) comment = "[selection \u201C" + popoverCtx.quote + "\u201D] " + comment;
    var el = popoverCtx.el;
    if (el && el.classList) { el.classList.add("vl-highlight"); el.setAttribute("data-vl", "1"); }
    annotations.push({
      selector: popoverCtx.selector,
      comment: comment || "",
      suggested_html: null
    });
    closePopover();
    render();
  }

  function closePopover() {
    if (popover && popover.parentNode) popover.parentNode.removeChild(popover);
    popover = null;
    popoverCtx = null;
  }

  function removeAnnotation(i) {
    annotations.splice(i, 1);
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
    setPickMode(false);
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
    document.addEventListener("DOMContentLoaded", function(){ setPickMode(pickMode); });
  } else {
    setPickMode(pickMode);
  }

  // Watchdog: artifact pages frequently rebuild document.body after load
  // (charts, hydration), which removes an appended panel. If our panel
  // vanishes and the review is still pending, re-attach and re-render it.
  // Listeners live on `document` (capture), so they survive body wipes.
  setInterval(function(){
    if (submitted) return;
    if (!document.getElementById("vl-panel")) {
      try { document.body.appendChild(panel); render(); } catch (e) {}
    }
  }, 800);
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
    fn overlay_uses_inline_popover_not_window_prompt() {
        // VRO-11.6: window.prompt was the "not interactive" complaint — the
        // overlay must use its own inline popover editor instead.
        assert!(
            !OVERLAY_SCRIPT.contains("window.prompt"),
            "window.prompt must not appear in the overlay"
        );
        assert!(
            OVERLAY_SCRIPT.contains("vl-popover"),
            "the inline popover element must exist"
        );
        assert!(
            OVERLAY_SCRIPT.contains("input.focus()"),
            "the popover input must auto-focus"
        );
    }

    #[test]
    fn overlay_supports_pick_mode_hover_and_selection() {
        // VRO-11.6 Oracle-style affordances: hover outline while
        // picking, text-selection annotation, removable note list, Esc exit.
        assert!(OVERLAY_SCRIPT.contains("vl-hover"));
        assert!(OVERLAY_SCRIPT.contains("onMouseUp"));
        assert!(OVERLAY_SCRIPT.contains("getSelection"));
        assert!(OVERLAY_SCRIPT.contains("vl-x"));
        assert!(OVERLAY_SCRIPT.contains("\"Escape\""));
        assert!(OVERLAY_SCRIPT.contains("Enter"));
    }

    #[test]
    fn overlay_pick_mode_is_on_by_default_and_boots_attached() {
        // VRO-11.7: interactivity must be immediate — the page opens in
        // pick mode (hover outline + click-to-annotate) with the listeners
        // attached at boot, no hidden button to discover.
        assert!(
            OVERLAY_SCRIPT.contains("var pickMode = true;"),
            "pick mode must default ON"
        );
        assert!(
            OVERLAY_SCRIPT.contains("setPickMode(pickMode);"),
            "boot must attach listeners for the default pick state"
        );
    }

    #[test]
    fn overlay_listeners_survive_body_wipes_and_panel_is_watchdogged() {
        // VRO-11.8: artifact dashboards often rebuild document.body after
        // load, destroying an appended panel and any body-level listeners.
        // The overlay must attach pick listeners to `document` (capture)
        // and re-attach its panel via a watchdog if it vanishes.
        assert!(
            !OVERLAY_SCRIPT.contains("document.body.addEventListener"),
            "no body-level listeners — they die with body wipes"
        );
        assert!(
            OVERLAY_SCRIPT.contains("document.addEventListener(\"click\", onBodyClick, true);"),
            "pick click listener must be document-level capture"
        );
        assert!(
            OVERLAY_SCRIPT.contains("setInterval"),
            "the panel watchdog must exist"
        );
        assert!(
            OVERLAY_SCRIPT.contains("document.getElementById(\"vl-panel\")"),
            "the watchdog must detect a removed panel"
        );
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
