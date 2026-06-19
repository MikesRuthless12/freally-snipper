//! Generic block (de)compressor: pick the smallest of a few owned strategies.
//!
//! Every payload in a `.fvid` stream — an intra frame, an inter delta, the audio
//! track — is wrapped by [`pack`]. It tries raw, Huffman, RLE, and RLE-then-
//! Huffman, then keeps whichever is smallest, prefixed with a one-byte tag. The
//! raw fallback guarantees a block never grows by more than that single tag byte.

use crate::{huffman, rle};

const TAG_RAW: u8 = 0;
const TAG_HUFFMAN: u8 = 1;
const TAG_RLE: u8 = 2;
const TAG_RLE_HUFFMAN: u8 = 3;

/// Compress `data` into a tagged block, choosing the smallest representation.
pub(crate) fn pack(data: &[u8]) -> Vec<u8> {
    let huffman = huffman::compress(data);
    let rle = rle::encode(data);
    let rle_huffman = huffman::compress(&rle);

    // (tag, blob) candidates; the raw blob is `data` itself.
    let mut best_tag = TAG_RAW;
    let mut best: &[u8] = data;
    for (tag, blob) in [
        (TAG_HUFFMAN, &huffman),
        (TAG_RLE, &rle),
        (TAG_RLE_HUFFMAN, &rle_huffman),
    ] {
        if blob.len() < best.len() {
            best_tag = tag;
            best = blob;
        }
    }

    let mut out = Vec::with_capacity(best.len() + 1);
    out.push(best_tag);
    out.extend_from_slice(best);
    out
}

/// Reverse [`pack`]. Returns `None` if the block is malformed.
pub(crate) fn unpack(block: &[u8]) -> Option<Vec<u8>> {
    let (&tag, rest) = block.split_first()?;
    match tag {
        TAG_RAW => Some(rest.to_vec()),
        TAG_HUFFMAN => huffman::decompress(rest),
        TAG_RLE => rle::decode(rest),
        TAG_RLE_HUFFMAN => rle::decode(&huffman::decompress(rest)?),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(data: &[u8]) {
        let packed = pack(data);
        // The raw fallback bounds growth to a single tag byte.
        assert!(packed.len() <= data.len() + 1);
        let back = unpack(&packed).expect("unpack");
        assert_eq!(back, data);
    }

    #[test]
    fn empty() {
        round_trip(&[]);
    }

    #[test]
    fn incompressible_is_stored_raw() {
        let mut state = 0xC0FF_EE00u32;
        let data: Vec<u8> = (0..512)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect();
        round_trip(&data);
        assert_eq!(pack(&data)[0], TAG_RAW);
    }

    #[test]
    fn long_run_uses_rle_family() {
        let data = vec![0u8; 20_000];
        round_trip(&data);
        let tag = pack(&data)[0];
        assert!(tag == TAG_RLE || tag == TAG_RLE_HUFFMAN);
        assert!(pack(&data).len() < data.len());
    }

    #[test]
    fn skewed_uses_huffman_family() {
        let data: Vec<u8> = (0..20_000).map(|i| (i % 4) as u8).collect();
        round_trip(&data);
        assert!(pack(&data).len() < data.len());
    }

    #[test]
    fn rejects_empty_block() {
        assert!(unpack(&[]).is_none());
    }

    #[test]
    fn rejects_unknown_tag() {
        assert!(unpack(&[99, 1, 2, 3]).is_none());
    }
}
