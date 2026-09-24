//! VRO-17 PR-4: the single implementation of the Settings → Voice
//! scope save. Included by the bin's settings host and by the lib test
//! seam — ONE implementation, never a second writer.

use std::path::Path;

#[cfg(feature = "voice-conversation")]
pub fn save_voice_scope(root: &Path, scope: &vesper_voice::VoiceScope) -> Result<(), String> {
    let dir = root.join(".agent-vesper");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("config.toml");
    // Preserve unknown tables/keys by rewriting only the [voice] block:
    // read existing text, replace-or-append the block. VRO-17 R3: the
    // selected speech engine and voice persist alongside enablement
    // (provider ids are scope-owned; absent selection = the baseline
    // system engine, preserving pre-R3 behavior).
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut block = format!(
        "[voice]\nenabled = {}\npartials = {}\n",
        scope.enabled, scope.partials
    );
    if let Some(tts) = &scope.tts {
        block.push_str(&format!("tts = \"{}\"\n", tts.as_str()));
    }
    if let Some(voice) = &scope.voice {
        block.push_str(&format!("voice = \"{voice}\"\n"));
    }
    // R16 clarified: per-stage execution policies persist alongside the
    // engine/voice (draft-only in Settings; independent per stage). The
    // `cpu` default keeps every pre-policy saved scope byte-compatible.
    block.push_str(&format!(
        "stt_compute = \"{}\"\ntts_compute = \"{}\"\n",
        scope.stt_compute.as_config_str(),
        scope.tts_compute.as_config_str()
    ));
    let updated = if let Some(start) = existing.find("[voice]") {
        // Replace through the end of the [voice] table (next table or EOF).
        let rest = &existing[start..];
        let end = rest[1..]
            .find("\n[")
            .map(|offset| start + 1 + offset + 1)
            .unwrap_or(existing.len());
        format!("{}{}{}", &existing[..start], block, &existing[end..])
    } else if existing.trim_end().is_empty() {
        block
    } else {
        format!("{}\n{}", existing.trim_end(), block)
    };
    std::fs::write(&path, updated).map_err(|e| e.to_string())
}

/// The production save/skip decision for the voice scope within the
/// Settings Save flow. Single source of truth used by the host AND the
/// regression test: voice saves on its OWN change, never gated behind
/// an unrelated panel's change.
pub fn voice_save_required(
    voice: &vesper_voice::VoiceScope,
    initial: &vesper_voice::VoiceScope,
    web_changed: bool,
) -> bool {
    // Deliberately independent of `web_changed`: the historical defect
    // nested this under the web-tools change.
    let _ = web_changed;
    voice != initial
}

/// The production dirty contribution of voice to the Settings draft
/// (historical defect #2: it was omitted entirely).
pub fn voice_dirty(voice: &vesper_voice::VoiceScope, initial: &vesper_voice::VoiceScope) -> bool {
    voice != initial
}
