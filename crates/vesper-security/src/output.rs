/// Completely drained output with bounded retained head/tail bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    retained: Vec<u8>,
    total_bytes: usize,
    truncated: bool,
}

impl BoundedOutput {
    /// Retains at most `limit` bytes while preserving both beginning and end.
    #[must_use]
    pub fn from_bytes(bytes: &[u8], limit: usize) -> Self {
        if bytes.len() <= limit {
            return Self {
                retained: bytes.to_vec(),
                total_bytes: bytes.len(),
                truncated: false,
            };
        }
        let head = limit / 2;
        let tail = limit - head;
        let mut retained = Vec::with_capacity(limit);
        retained.extend_from_slice(&bytes[..head]);
        retained.extend_from_slice(&bytes[bytes.len() - tail..]);
        Self {
            retained,
            total_bytes: bytes.len(),
            truncated: true,
        }
    }

    /// Retained bounded bytes.
    #[must_use]
    pub fn retained(&self) -> &[u8] {
        &self.retained
    }

    /// Total bytes observed before bounding.
    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Whether bytes were omitted from the retained representation.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_preserves_head_tail_and_total() {
        let output = BoundedOutput::from_bytes(b"0123456789", 6);
        assert_eq!(output.retained(), b"012789");
        assert_eq!(output.total_bytes(), 10);
        assert!(output.truncated());
    }
}
