//! Canonical audio contract: validated sample-aligned frames.
//!
//! v1 fixes the wire format at i16 little-endian, 16 000 Hz, mono
//! (PRD §2.2). Two layers are deliberately distinct:
//!
//! - **Transport bytes** arrive at arbitrary boundaries at the host's
//!   capture/playback edges. They are never a [`PcmFrame`].
//! - A [`PcmFrame`] is a *validated, sample-aligned* unit: even byte
//!   count, exactly the canonical format. It is produced only by
//!   [`PcmReassembler`], which carries an unmatched odd byte to the next
//!   chunk (the voice-oracle playback lesson) and reports an unmatched
//!   final byte as truncation instead of inventing or dropping a sample.
//!
//! The core performs no resampling: a device or engine producing a
//! different rate is resampled at the host capture boundary before frames
//! enter this crate, and adapters requiring other formats say so in their
//! descriptors.

use serde::{Deserialize, Serialize};

/// The one canonical v1 audio format.
pub const CANONICAL_SAMPLE_RATE_HZ: u32 = 16_000;
/// The one canonical v1 channel count.
pub const CANONICAL_CHANNELS: u16 = 1;

/// Describes and validates the canonical audio format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFormat {
    /// Samples per second; must equal [`CANONICAL_SAMPLE_RATE_HZ`].
    pub sample_rate_hz: u32,
    /// Channel count; must equal [`CANONICAL_CHANNELS`].
    pub channels: u16,
    /// Bits per sample; must be 16 (i16).
    pub bits_per_sample: u16,
}

impl AudioFormat {
    /// The canonical v1 format.
    #[must_use]
    pub const fn canonical() -> Self {
        Self {
            sample_rate_hz: CANONICAL_SAMPLE_RATE_HZ,
            channels: CANONICAL_CHANNELS,
            bits_per_sample: 16,
        }
    }

    /// Returns `Ok(())` when this is exactly the canonical format.
    ///
    /// # Errors
    ///
    /// [`super::VoiceError::InvalidInput`] naming the offending field for
    /// any non-canonical value. Resampling is a host/adapter
    /// responsibility performed before frames enter the core; the core
    /// never silently accepts or converts other formats.
    pub fn validate(&self) -> Result<(), super::VoiceError> {
        if self.sample_rate_hz != CANONICAL_SAMPLE_RATE_HZ {
            return Err(super::VoiceError::InvalidInput(
                "audio sample rate must be 16000 Hz (resample at the host capture boundary)".into(),
            ));
        }
        if self.channels != CANONICAL_CHANNELS {
            return Err(super::VoiceError::InvalidInput(
                "audio must be mono (downmix at the host capture boundary)".into(),
            ));
        }
        if self.bits_per_sample != 16 {
            return Err(super::VoiceError::InvalidInput(
                "audio must be signed 16-bit samples".into(),
            ));
        }
        Ok(())
    }
}

/// A validated, sample-aligned PCM audio frame in the canonical format.
///
/// Constructed only through [`PcmReassembler`] (or, in tests, from
/// already-aligned bytes); an odd-length payload is a construction error,
/// never a silent truncation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmFrame {
    bytes: Vec<u8>,
}

impl PcmFrame {
    /// Builds a frame from bytes that are already sample-aligned.
    ///
    /// # Errors
    ///
    /// [`super::VoiceError::InvalidInput`] when `bytes.len()` is odd: raw
    /// transport chunks must go through [`PcmReassembler`] instead.
    pub fn from_aligned(bytes: Vec<u8>) -> Result<Self, super::VoiceError> {
        if !bytes.len().is_multiple_of(2) {
            return Err(super::VoiceError::InvalidInput(
                "pcm frame payload must be sample-aligned (even byte count)".into(),
            ));
        }
        Ok(Self { bytes })
    }

    /// The raw sample-aligned payload.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Number of complete i16 samples in this frame.
    #[must_use]
    pub fn sample_count(&self) -> usize {
        self.bytes.len() / 2
    }

    /// Duration of this frame in whole milliseconds (floor).
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        (self.sample_count() as u64) * 1000 / u64::from(CANONICAL_SAMPLE_RATE_HZ)
    }
}

/// Turns arbitrary transport byte chunks into validated frames.
///
/// Frames may split at any byte boundary; the reassembler groups bytes
/// into aligned frames, carrying an unmatched odd byte forward. A capture
/// that ends with a carried byte has lost half a sample: [`finish`]
/// reports it as [`super::VoiceError::Truncated`] so truncation stays
/// observable.
///
/// [`finish`]: PcmReassembler::finish
#[derive(Debug, Default)]
pub struct PcmReassembler {
    carried: Option<u8>,
}

impl PcmReassembler {
    /// A reassembler with no carried state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Accepts one transport chunk, emitting zero or more aligned frames.
    ///
    /// # Errors
    ///
    /// [`super::VoiceError::InvalidInput`] only for empty input (a
    /// transport layer delivering zero bytes is a protocol fault worth
    /// surfacing, not a frame).
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<PcmFrame>, super::VoiceError> {
        if chunk.is_empty() {
            return Err(super::VoiceError::InvalidInput(
                "transport delivered an empty audio chunk".into(),
            ));
        }
        let mut stream: Vec<u8> = Vec::with_capacity(chunk.len() + 1);
        if let Some(byte) = self.carried.take() {
            stream.push(byte);
        }
        stream.extend_from_slice(chunk);
        let aligned_len = stream.len() - stream.len() % 2;
        if stream.len() > aligned_len {
            self.carried = stream.last().copied();
        }
        // One byte-preserving frame per push: the reassembler groups all
        // currently aligned bytes; sample boundaries are respected.
        let mut frames: Vec<PcmFrame> = Vec::new();
        if aligned_len > 0 {
            frames.push(
                PcmFrame::from_aligned(stream[..aligned_len].to_vec()).map_err(|_| {
                    super::VoiceError::InvalidInput(
                        "reassembler produced a misaligned frame".into(),
                    )
                })?,
            );
        }
        Ok(frames)
    }

    /// Ends a capture, reporting an unmatched carried byte as truncation.
    ///
    /// # Errors
    ///
    /// [`super::VoiceError::Truncated`] when a lone byte remains — the
    /// capture lost half a sample and the fact must be observable rather
    /// than silently dropped.
    pub fn finish(&mut self) -> Result<(), super::VoiceError> {
        if self.carried.take().is_some() {
            return Err(super::VoiceError::Truncated);
        }
        Ok(())
    }

    /// Whether a byte is currently carried (diagnostics only).
    #[must_use]
    pub fn has_carried_byte(&self) -> bool {
        self.carried.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_format_validates() {
        assert!(AudioFormat::canonical().validate().is_ok());
        assert!(
            AudioFormat {
                sample_rate_hz: 48_000,
                ..AudioFormat::canonical()
            }
            .validate()
            .is_err()
        );
        assert!(
            AudioFormat {
                channels: 2,
                ..AudioFormat::canonical()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn odd_payload_is_not_a_frame() {
        assert!(PcmFrame::from_aligned(vec![1, 2, 3]).is_err());
        let frame = PcmFrame::from_aligned(vec![1, 2, 3, 4]).unwrap();
        assert_eq!(frame.sample_count(), 2);
        assert_eq!(frame.duration_ms(), 0);
    }

    #[test]
    fn arbitrary_boundaries_reassemble_to_aligned_frames() {
        let mut re = PcmReassembler::new();
        let first = re.push(&[0x01, 0x02, 0x03]).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].bytes(), &[0x01, 0x02]);
        assert!(re.has_carried_byte());
        let second = re.push(&[0x04, 0x05, 0x06, 0x07]).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].bytes(), &[0x03, 0x04, 0x05, 0x06]);
        assert!(re.has_carried_byte());
        assert!(matches!(re.finish(), Err(crate::VoiceError::Truncated)));
    }

    #[test]
    fn clean_finish_has_no_truncation() {
        let mut re = PcmReassembler::new();
        let frames = re.push(&[9, 9, 9, 9, 9, 9]).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].sample_count(), 3);
        assert!(re.finish().is_ok());
        assert!(!re.has_carried_byte());
    }

    #[test]
    fn empty_chunk_is_rejected() {
        let mut re = PcmReassembler::new();
        assert!(re.push(&[]).is_err());
    }

    #[test]
    fn duration_is_computed_from_samples() {
        // 1600 samples = 100 ms at 16 kHz.
        let frame = PcmFrame::from_aligned(vec![0u8; 3200]).unwrap();
        assert_eq!(frame.duration_ms(), 100);
    }
}
