//! Minimal MSB-first bit writer / reader for the Huffman entropy stage.
//!
//! Owned and dependency-free, like the rest of `freally-video`. Bits are packed
//! most-significant-bit first within each byte, which is the order the canonical
//! Huffman coder ([`crate::huffman`]) expects.

/// Accumulates individual bits into a byte buffer, MSB-first.
pub(crate) struct BitWriter {
    bytes: Vec<u8>,
    /// Bits filled so far in `cur`, shifted in from the right (0..=7).
    cur: u8,
    nbits: u8,
}

impl BitWriter {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            cur: 0,
            nbits: 0,
        }
    }

    /// Write the low `count` bits of `value`, most-significant first.
    ///
    /// `count` must be in `0..=32`; higher bits of `value` are ignored.
    pub(crate) fn write_bits(&mut self, value: u32, count: u8) {
        let mut i = count;
        while i > 0 {
            i -= 1;
            let bit = ((value >> i) & 1) as u8;
            self.cur = (self.cur << 1) | bit;
            self.nbits += 1;
            if self.nbits == 8 {
                self.bytes.push(self.cur);
                self.cur = 0;
                self.nbits = 0;
            }
        }
    }

    /// Flush any partial byte (zero-padded on the right) and return the buffer.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.cur <<= 8 - self.nbits;
            self.bytes.push(self.cur);
        }
        self.bytes
    }
}

/// Reads individual bits from a byte slice, MSB-first (the inverse of
/// [`BitWriter`]).
pub(crate) struct BitReader<'a> {
    bytes: &'a [u8],
    byte_pos: usize,
    /// Index of the next bit to read in the current byte (0 = MSB).
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Read a single bit, or `None` once the buffer is exhausted.
    pub(crate) fn read_bit(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.byte_pos)?;
        let bit = (byte >> (7 - self.bit_pos)) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Some(bit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_known_bit_pattern() {
        let mut w = BitWriter::new();
        // 0b101, then 0b0000_1111 (4 bits => 1111), then a single 0 bit.
        w.write_bits(0b101, 3);
        w.write_bits(0b1111, 4);
        w.write_bits(0, 1);
        let bytes = w.finish();
        // 8 bits exactly => one byte: 1011_1110.
        assert_eq!(bytes, vec![0b1011_1110]);

        let mut r = BitReader::new(&bytes);
        let got: Vec<u8> = (0..8).map(|_| r.read_bit().unwrap()).collect();
        assert_eq!(got, vec![1, 0, 1, 1, 1, 1, 1, 0]);
        // Past the end yields None.
        assert_eq!(r.read_bit(), None);
    }

    #[test]
    fn wide_values_round_trip() {
        let mut w = BitWriter::new();
        w.write_bits(0x00AB_CDEF, 24);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let mut v = 0u32;
        for _ in 0..24 {
            v = (v << 1) | r.read_bit().unwrap() as u32;
        }
        assert_eq!(v, 0x00AB_CDEF);
    }

    #[test]
    fn padding_bits_are_zero() {
        let mut w = BitWriter::new();
        w.write_bits(1, 1); // one bit set => 1000_0000 after padding.
        assert_eq!(w.finish(), vec![0b1000_0000]);
    }
}
