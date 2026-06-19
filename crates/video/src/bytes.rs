//! Little-endian byte reading/writing helpers for the `.fvid` container.

use crate::{Result, VideoError};

/// A bounds-checked forward reader over a byte slice. Every read returns
/// [`VideoError::Truncated`] rather than panicking when the buffer runs out.
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Borrow the next `n` bytes, advancing the cursor.
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(VideoError::Truncated)?;
        let slice = self.data.get(self.pos..end).ok_or(VideoError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
}

pub(crate) fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_then_reads_back() {
        let mut buf = Vec::new();
        buf.push(0xABu8);
        put_u16(&mut buf, 0x1234);
        put_u32(&mut buf, 0x89AB_CDEF);
        put_u64(&mut buf, 0x0102_0304_0506_0708);

        let mut cur = Cursor::new(&buf);
        assert_eq!(cur.read_u8().unwrap(), 0xAB);
        assert_eq!(cur.read_u16().unwrap(), 0x1234);
        assert_eq!(cur.read_u32().unwrap(), 0x89AB_CDEF);
        assert_eq!(cur.read_u64().unwrap(), 0x0102_0304_0506_0708);
    }

    #[test]
    fn take_past_end_errors() {
        let mut cur = Cursor::new(&[1, 2, 3]);
        assert!(cur.take(4).is_err());
        // The successful prefix is still readable.
        assert_eq!(cur.take(3).unwrap(), &[1, 2, 3]);
        assert!(cur.read_u8().is_err());
    }
}
