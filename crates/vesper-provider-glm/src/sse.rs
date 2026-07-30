use thiserror::Error;

/// Maximum one SSE line, excluding its line ending.
pub const MAX_SSE_LINE_BYTES: usize = 256 * 1024;
/// Maximum one `data:` payload.
pub const MAX_SSE_EVENT_BYTES: usize = 256 * 1024;
/// Maximum accumulated tool name.
pub const MAX_TOOL_NAME_BYTES: usize = 128;
/// Maximum accumulated tool arguments per call.
pub const MAX_TOOL_ARGUMENT_BYTES: usize = 1024 * 1024;
/// Maximum bounded provider metadata encoded size.
pub const MAX_PROVIDER_METADATA_BYTES: usize = 64 * 1024;
/// Maximum non-success response prefix retained for classification.
pub const MAX_ERROR_BODY_BYTES: usize = 500;

/// One source-compatible SSE line outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseFrame {
    /// One non-empty `data:` payload.
    Data(String),
    /// Provider sent `[DONE]`.
    Done,
}

/// Bounded streaming parser failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SseError {
    /// One line exceeded its bound.
    #[error("SSE line exceeded its bound")]
    LineTooLarge,
    /// One data event exceeded its bound.
    #[error("SSE event exceeded its bound")]
    EventTooLarge,
    /// A data line was not valid UTF-8.
    #[error("SSE data was not valid UTF-8")]
    InvalidUtf8,
}

/// Incremental, arbitrary-byte-chunk SSE parser.
///
/// Frozen Python compatibility is intentionally line-based: comments, blank
/// lines, and non-`data:` fields are ignored; malformed JSON is handled by the
/// GLM chunk normalizer rather than this framing layer.
#[derive(Debug, Default)]
pub struct SseParser {
    pending: Vec<u8>,
    terminal: bool,
}

impl SseParser {
    /// Adds arbitrary transport bytes and returns complete frames.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseFrame>, SseError> {
        if self.terminal {
            return Ok(Vec::new());
        }
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > MAX_SSE_LINE_BYTES && !self.pending.contains(&b'\n') {
            return Err(SseError::LineTooLarge);
        }
        let mut frames = Vec::new();
        while let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line = self.pending.drain(..=index).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.len() > MAX_SSE_LINE_BYTES {
                return Err(SseError::LineTooLarge);
            }
            if let Some(frame) = parse_line(&line)? {
                self.terminal |= frame == SseFrame::Done;
                frames.push(frame);
                if self.terminal {
                    self.pending.clear();
                    break;
                }
            }
        }
        if self.pending.len() > MAX_SSE_LINE_BYTES {
            return Err(SseError::LineTooLarge);
        }
        Ok(frames)
    }

    /// Processes a final unterminated line at EOF.
    pub fn finish(&mut self) -> Result<Vec<SseFrame>, SseError> {
        if self.terminal || self.pending.is_empty() {
            self.pending.clear();
            return Ok(Vec::new());
        }
        let mut line = std::mem::take(&mut self.pending);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.len() > MAX_SSE_LINE_BYTES {
            return Err(SseError::LineTooLarge);
        }
        Ok(parse_line(&line)?.into_iter().collect())
    }
}

fn parse_line(line: &[u8]) -> Result<Option<SseFrame>, SseError> {
    if line.is_empty() || line.starts_with(b":") || !line.starts_with(b"data:") {
        return Ok(None);
    }
    let data = std::str::from_utf8(&line[5..])
        .map_err(|_| SseError::InvalidUtf8)?
        .trim();
    if data.is_empty() {
        return Ok(None);
    }
    if data.len() > MAX_SSE_EVENT_BYTES {
        return Err(SseError::EventTooLarge);
    }
    if data == "[DONE]" {
        Ok(Some(SseFrame::Done))
    } else {
        Ok(Some(SseFrame::Data(data.to_owned())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_chunk_boundary_preserves_utf8_crlf_and_ignored_lines() {
        let input =
            ": keepalive\r\nevent: message\r\ndata: {\"text\":\"思考\"}\r\n\r\ndata: [DONE]\n";
        let expected = vec![SseFrame::Data("{\"text\":\"思考\"}".into()), SseFrame::Done];
        for split in 0..=input.len() {
            let mut parser = SseParser::default();
            let mut frames = parser.push(&input.as_bytes()[..split]).unwrap();
            frames.extend(parser.push(&input.as_bytes()[split..]).unwrap());
            frames.extend(parser.finish().unwrap());
            assert_eq!(frames, expected, "split at {split}");
        }
    }

    #[test]
    fn bounds_apply_before_unlimited_accumulation() {
        let mut parser = SseParser::default();
        assert_eq!(
            parser.push(&vec![b'x'; MAX_SSE_LINE_BYTES + 1]),
            Err(SseError::LineTooLarge)
        );
    }
}
