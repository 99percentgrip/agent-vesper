//! The pinned phoneme→ID vocabulary (`tokenizer.json` at the pack's
//! pinned revision). Parsed once per adapter; never re-read per unit.
//!
//! The vocabulary is a pack-owned input contract (the file is downloaded,
//! digest-verified, and versioned with the pack) — not embedded source,
//! so a pack update carries its own vocabulary without a code change.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use vesper_voice::error::VoiceError;

use crate::pack;

/// A parsed phoneme vocabulary (char → model input ID).
#[derive(Debug, Clone)]
pub struct PhonemeVocab {
    map: HashMap<char, i64>,
}

impl PhonemeVocab {
    /// Parses the `{"model": {"vocab": {…}}}` tokenizer-JSON shape.
    ///
    /// # Errors
    /// [`VoiceError::InvalidInput`] on malformed JSON or non-char keys.
    pub fn from_json_str(json: &str) -> Result<Self, VoiceError> {
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|_| VoiceError::InvalidInput("vocabulary is not valid JSON".into()))?;
        let vocab = value
            .get("model")
            .and_then(|model| model.get("vocab"))
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| VoiceError::InvalidInput("vocabulary missing model.vocab".into()))?;
        let mut map = HashMap::with_capacity(vocab.len());
        for (key, id) in vocab {
            let id = id.as_i64().ok_or_else(|| {
                VoiceError::InvalidInput("vocabulary id is not an integer".into())
            })?;
            let mut chars = key.chars();
            let (first, second) = (chars.next(), chars.next());
            match (first, second) {
                (Some(ch), None) => {
                    map.insert(ch, id);
                }
                _ => {
                    return Err(VoiceError::InvalidInput(
                        "vocabulary key is not exactly one char".into(),
                    ));
                }
            }
        }
        if map.is_empty() {
            return Err(VoiceError::InvalidInput("vocabulary is empty".into()));
        }
        Ok(Self { map })
    }

    /// The model ID for one phoneme character, if represented.
    #[must_use]
    pub fn id_for(&self, ch: char) -> Option<i64> {
        self.map.get(&ch).copied()
    }

    /// Vocabulary size (sanity surface for diagnostics).
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// True when empty (constructed vocabularies are never empty; kept
    /// for explicitness in ready-state checks).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Loads the pack vocabulary from its installed `tokenizer.json`.
    ///
    /// # Errors
    /// [`VoiceError::Unavailable`] when the pack or the file is absent;
    /// [`VoiceError::InvalidInput`] on malformed content.
    pub fn load_from_pack(root: &Path) -> Result<Self, VoiceError> {
        let path = root.join(pack::ASSET_VOCAB.installed_name);
        let json = std::fs::read_to_string(&path).map_err(|_| VoiceError::Unavailable {
            provider: crate::provider_id(),
            reason: vesper_domain::BoundedString::new("voice pack vocabulary is missing")
                .unwrap_or_else(|_| {
                    vesper_domain::BoundedString::new("vocabulary missing").expect("fits")
                }),
        })?;
        Self::from_json_str(&json)
    }

    /// Loads the vocabulary only from a fully verified pack (the readiness
    /// gate calls this; it fails closed on any pack problem).
    ///
    /// # Errors
    /// [`VoiceError::Unavailable`] with the pack problem description.
    pub fn load_verified(root: &Path) -> Result<Self, VoiceError> {
        match pack::assess(root) {
            Ok(Some(problem)) => Err(VoiceError::Unavailable {
                provider: crate::provider_id(),
                reason: vesper_domain::BoundedString::new(problem.description()).unwrap_or_else(
                    |_| vesper_domain::BoundedString::new("voice pack not ready").expect("fits"),
                ),
            }),
            Ok(None) => Self::load_from_pack(root),
            Err(error) => Err(error),
        }
    }
}

/// Shared vocabulary handle (one parse per adapter lifetime).
pub type SharedVocab = Arc<PhonemeVocab>;

/// Sentinel for readiness plumbing (the pack problem type is reused by
/// the TUI assessment; kept here so the TUI needs only this crate).
#[allow(dead_code)]
pub use pack::PackProblem as PackProblemAlias;
