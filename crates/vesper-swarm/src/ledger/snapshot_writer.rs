//! Memory-only bounded serializer sink; never opens a file or socket.
use std::io::{self, Write};

pub(super) struct SnapshotWriter {
    pub bytes: Vec<u8>,
    limit: usize,
}
impl SnapshotWriter {
    pub fn new(bytes: Vec<u8>, limit: usize) -> Self {
        Self { bytes, limit }
    }
}
impl Write for SnapshotWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let length = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|length| *length <= self.limit)
            .ok_or_else(|| io::Error::other("snapshot byte limit exceeded"))?;
        if length > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(length)
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| io::Error::other("snapshot allocation refused"))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_limit_is_checked_before_appending() {
        let mut writer = SnapshotWriter::new(vec![1, 2], 4);
        writer.write_all(&[3, 4]).unwrap();
        assert!(writer.write_all(&[5]).is_err());
        assert_eq!(writer.bytes, [1, 2, 3, 4]);
    }
}
