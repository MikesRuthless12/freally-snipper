//! Canonical Huffman entropy coder over bytes — owned and dependency-free.
//!
//! Used as one stage of [`crate::pack`]. The compressed blob is laid out as:
//!
//! ```text
//! [u64 symbol count][256 × u8 code lengths][bitstream]
//! ```
//!
//! Code lengths are length-limited to [`MAX_CODE_LEN`] bits so every canonical
//! code fits in a `u32` and decoding stays bounded. Limiting is done by halving
//! the frequency table and rebuilding until the longest code is short enough —
//! a simple, well-known technique that converges (equal weights give a balanced
//! tree of depth `ceil(log2(256)) = 8`).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::bitio::{BitReader, BitWriter};

/// Maximum Huffman code length, in bits. Keeps codes within a `u32` and bounds
/// the decode loop.
const MAX_CODE_LEN: usize = 24;

/// Count how often each byte value occurs in `data`.
fn frequencies(data: &[u8]) -> [u64; 256] {
    let mut freq = [0u64; 256];
    for &b in data {
        freq[b as usize] += 1;
    }
    freq
}

/// One Huffman pass: optimal code lengths for the given frequencies, with no
/// length limit applied yet. Unused symbols (frequency 0) get length 0.
fn huffman_lengths(freq: &[u64; 256]) -> [u8; 256] {
    let mut lengths = [0u8; 256];
    let used: Vec<usize> = (0..256).filter(|&i| freq[i] > 0).collect();
    match used.len() {
        0 => return lengths,
        // A single symbol still needs one bit (a zero-length code is undecodable).
        1 => {
            lengths[used[0]] = 1;
            return lengths;
        }
        _ => {}
    }

    /// Arena node: a leaf carries `sym >= 0`; an internal node carries children.
    struct Node {
        sym: i32,
        left: usize,
        right: usize,
    }
    let mut nodes: Vec<Node> = Vec::with_capacity(used.len() * 2);
    // Min-heap keyed by (weight, insertion order) for deterministic output.
    let mut heap: BinaryHeap<Reverse<(u64, u64, usize)>> = BinaryHeap::new();
    let mut seq = 0u64;
    for &s in &used {
        let idx = nodes.len();
        nodes.push(Node {
            sym: s as i32,
            left: 0,
            right: 0,
        });
        heap.push(Reverse((freq[s], seq, idx)));
        seq += 1;
    }
    while heap.len() > 1 {
        let Reverse((w1, _, i1)) = heap.pop().expect("heap has >= 2 nodes");
        let Reverse((w2, _, i2)) = heap.pop().expect("heap has >= 2 nodes");
        let idx = nodes.len();
        nodes.push(Node {
            sym: -1,
            left: i1,
            right: i2,
        });
        heap.push(Reverse((w1 + w2, seq, idx)));
        seq += 1;
    }
    let Reverse((_, _, root)) = heap.pop().expect("one root node remains");

    // Iteratively walk the tree, recording each leaf's depth as its code length.
    let mut stack = vec![(root, 0u32)];
    while let Some((idx, depth)) = stack.pop() {
        let node = &nodes[idx];
        if node.sym >= 0 {
            lengths[node.sym as usize] = depth.min(255) as u8;
        } else {
            stack.push((node.left, depth + 1));
            stack.push((node.right, depth + 1));
        }
    }
    lengths
}

/// Length-limited Huffman code lengths: rebuild with progressively flattened
/// frequencies until the longest code fits in [`MAX_CODE_LEN`] bits.
fn code_lengths(freq: &[u64; 256]) -> [u8; 256] {
    let mut scaled = *freq;
    loop {
        let lengths = huffman_lengths(&scaled);
        let longest = lengths.iter().copied().max().unwrap_or(0) as usize;
        if longest <= MAX_CODE_LEN {
            return lengths;
        }
        // Halve every used weight (keeping it non-zero) to shrink the spread.
        for w in scaled.iter_mut() {
            if *w > 0 {
                *w = (*w >> 1) | 1;
            }
        }
    }
}

/// Assign canonical codes from code lengths (the DEFLATE scheme).
fn canonical_codes(lengths: &[u8; 256]) -> [u32; 256] {
    let mut bl_count = [0u32; MAX_CODE_LEN + 1];
    for &l in lengths.iter() {
        if l > 0 {
            bl_count[l as usize] += 1;
        }
    }
    let mut next_code = [0u32; MAX_CODE_LEN + 1];
    let mut code = 0u32;
    for bits in 1..=MAX_CODE_LEN {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
    }
    let mut codes = [0u32; 256];
    for (sym, &l) in lengths.iter().enumerate() {
        if l > 0 {
            codes[sym] = next_code[l as usize];
            next_code[l as usize] += 1;
        }
    }
    codes
}

/// Compress `data` into a self-describing Huffman blob.
pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
    let freq = frequencies(data);
    let lengths = code_lengths(&freq);
    let codes = canonical_codes(&lengths);

    let mut writer = BitWriter::new();
    for &b in data {
        writer.write_bits(codes[b as usize], lengths[b as usize]);
    }
    let bitstream = writer.finish();

    let mut out = Vec::with_capacity(8 + 256 + bitstream.len());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&lengths[..]);
    out.extend_from_slice(&bitstream);
    out
}

/// Decompress a blob produced by [`compress`]. Returns `None` if the blob is
/// malformed or truncated.
pub(crate) fn decompress(blob: &[u8]) -> Option<Vec<u8>> {
    const HEADER: usize = 8 + 256;
    if blob.len() < HEADER {
        return None;
    }
    let count = u64::from_le_bytes(blob[0..8].try_into().ok()?) as usize;
    let mut lengths = [0u8; 256];
    lengths.copy_from_slice(&blob[8..HEADER]);
    let bitstream = &blob[HEADER..];

    // Rebuild the canonical decode tables from the transmitted code lengths.
    let mut bl_count = [0u32; MAX_CODE_LEN + 1];
    for &l in lengths.iter() {
        let l = l as usize;
        if l > 0 {
            if l > MAX_CODE_LEN {
                return None;
            }
            bl_count[l] += 1;
        }
    }
    let mut first_code = [0u32; MAX_CODE_LEN + 1];
    let mut first_index = [0u32; MAX_CODE_LEN + 1];
    let mut code = 0u32;
    let mut index = 0u32;
    for bits in 1..=MAX_CODE_LEN {
        code = (code + bl_count[bits - 1]) << 1;
        first_code[bits] = code;
        first_index[bits] = index;
        index += bl_count[bits];
    }
    // Symbols ordered by (length, value) — matching the canonical assignment.
    let mut symbols: Vec<u16> = Vec::with_capacity(index as usize);
    for bits in 1..=MAX_CODE_LEN {
        for (sym, &l) in lengths.iter().enumerate() {
            if l as usize == bits {
                symbols.push(sym as u16);
            }
        }
    }

    let mut reader = BitReader::new(bitstream);
    // Each decoded symbol consumes at least one bit, so the output can't exceed
    // `bitstream.len() * 8` symbols — cap the reservation by that so a hostile
    // `count` (up to u64::MAX) can't pre-allocate unbounded memory. The decode
    // loop still terminates early via `read_bit()? -> None` when the bits run out.
    let mut out = Vec::with_capacity(count.min(bitstream.len().saturating_mul(8)));
    for _ in 0..count {
        let mut code = 0u32;
        let mut len = 0usize;
        loop {
            let bit = reader.read_bit()?;
            code = (code << 1) | bit as u32;
            len += 1;
            if len > MAX_CODE_LEN {
                return None;
            }
            if bl_count[len] > 0 {
                let offset = code.wrapping_sub(first_code[len]);
                if offset < bl_count[len] {
                    let sym_idx = (first_index[len] + offset) as usize;
                    out.push(*symbols.get(sym_idx)? as u8);
                    break;
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(data: &[u8]) {
        let blob = compress(data);
        let back = decompress(&blob).expect("decompress");
        assert_eq!(back, data);
    }

    #[test]
    fn empty_input() {
        round_trip(&[]);
    }

    #[test]
    fn single_distinct_symbol() {
        round_trip(&[7u8; 1000]);
    }

    #[test]
    fn two_symbols() {
        let data: Vec<u8> = (0..1000).map(|i| if i % 3 == 0 { 1 } else { 2 }).collect();
        round_trip(&data);
    }

    #[test]
    fn all_256_symbols() {
        let data: Vec<u8> = (0..=255).cycle().take(4096).collect();
        round_trip(&data);
    }

    #[test]
    fn skewed_distribution() {
        // Mostly zeros with a long tail — exercises the length limiter.
        let mut data = vec![0u8; 50_000];
        for (i, b) in data.iter_mut().enumerate() {
            if i.is_multiple_of(257) {
                *b = (i % 256) as u8;
            }
        }
        round_trip(&data);
    }

    #[test]
    fn pseudo_random() {
        // A cheap deterministic PRNG (no external crate): linear congruential.
        let mut state = 0x1234_5678u32;
        let data: Vec<u8> = (0..10_000)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect();
        round_trip(&data);
    }

    #[test]
    fn compresses_repetitive_data() {
        let data = vec![42u8; 100_000];
        let blob = compress(&data);
        assert!(blob.len() < data.len(), "expected compression to shrink");
    }

    #[test]
    fn rejects_truncated_blob() {
        assert!(decompress(&[0, 1, 2]).is_none());
    }
}
