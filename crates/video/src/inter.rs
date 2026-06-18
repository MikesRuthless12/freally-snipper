//! Inter-frame delta coder — stores only the regions that changed.
//!
//! Owned and dependency-free. The frame is divided into [`TILE`]×[`TILE`] tiles;
//! tiles identical to the previous frame are skipped entirely, and changed tiles
//! are stored as a per-byte wrapping difference from the previous frame. Those
//! deltas are mostly zero, so the downstream RLE+Huffman stage ([`crate::pack`])
//! compresses them well. Reconstruction is exact (lossless).
//!
//! Stream layout (before [`crate::pack`]): `[tile u8][dirty bitmap][delta bytes…]`.

/// Tile edge length, in pixels.
const TILE: u32 = 16;

/// Encode `cur` as a delta from `prev` (both `width * height * 4` RGBA bytes).
///
/// Returns the delta stream plus `(dirty_tiles, total_tiles)` so the caller can
/// detect a near-total change (a scene cut) and fall back to a keyframe.
pub(crate) fn encode(prev: &[u8], cur: &[u8], width: u32, height: u32) -> (Vec<u8>, usize, usize) {
    let tiles_x = width.div_ceil(TILE);
    let tiles_y = height.div_ceil(TILE);
    let total = (tiles_x * tiles_y) as usize;

    let mut dirty = vec![false; total];
    let mut dirty_count = 0;
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let ti = (ty * tiles_x + tx) as usize;
            if tile_differs(prev, cur, width, height, tx, ty) {
                dirty[ti] = true;
                dirty_count += 1;
            }
        }
    }

    let bitmap_len = total.div_ceil(8);
    let mut out = Vec::with_capacity(1 + bitmap_len + dirty_count * (TILE * TILE * 4) as usize);
    out.push(TILE as u8);

    let mut bitmap = vec![0u8; bitmap_len];
    for (ti, &d) in dirty.iter().enumerate() {
        if d {
            bitmap[ti / 8] |= 1 << (ti % 8);
        }
    }
    out.extend_from_slice(&bitmap);

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let ti = (ty * tiles_x + tx) as usize;
            if !dirty[ti] {
                continue;
            }
            for_each_tile_row(width, height, tx, ty, |start, len| {
                let p = start * 4;
                let cur_row = &cur[p..p + len * 4];
                let prev_row = &prev[p..p + len * 4];
                for (c, pv) in cur_row.iter().zip(prev_row) {
                    out.push(c.wrapping_sub(*pv));
                }
            });
        }
    }
    (out, dirty_count, total)
}

/// Reconstruct `cur` from `prev` and a delta stream produced by [`encode`].
/// Returns `None` if the stream is malformed or truncated.
pub(crate) fn decode(stream: &[u8], prev: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let tile = *stream.first()? as u32;
    if tile == 0 {
        return None;
    }
    let tiles_x = width.div_ceil(tile);
    let tiles_y = height.div_ceil(tile);
    let total = (tiles_x * tiles_y) as usize;
    let bitmap_len = total.div_ceil(8);

    let bitmap = stream.get(1..1 + bitmap_len)?;
    let mut deltas = &stream[1 + bitmap_len..];

    // Clean tiles are already correct because we start from a copy of `prev`.
    let mut cur = prev.to_vec();
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let ti = (ty * tiles_x + tx) as usize;
            if (bitmap[ti / 8] >> (ti % 8)) & 1 == 0 {
                continue;
            }
            let mut failed = false;
            for_each_tile_row_with(tile, width, height, tx, ty, |start, len| {
                let p = start * 4;
                let n = len * 4;
                if deltas.len() < n {
                    failed = true;
                    return;
                }
                let (chunk, rest) = deltas.split_at(n);
                deltas = rest;
                for (dst, d) in cur[p..p + n].iter_mut().zip(chunk) {
                    *dst = dst.wrapping_add(*d);
                }
            });
            if failed {
                return None;
            }
        }
    }
    Some(cur)
}

/// Does tile `(tx, ty)` differ between `prev` and `cur`?
fn tile_differs(prev: &[u8], cur: &[u8], width: u32, height: u32, tx: u32, ty: u32) -> bool {
    let mut differs = false;
    for_each_tile_row(width, height, tx, ty, |start, len| {
        let p = start * 4;
        if cur[p..p + len * 4] != prev[p..p + len * 4] {
            differs = true;
        }
    });
    differs
}

/// Invoke `f(pixel_start, pixel_len)` for each clipped row of tile `(tx, ty)`,
/// using the fixed encoder [`TILE`] size.
fn for_each_tile_row(width: u32, height: u32, tx: u32, ty: u32, f: impl FnMut(usize, usize)) {
    for_each_tile_row_with(TILE, width, height, tx, ty, f);
}

/// Like [`for_each_tile_row`] but with an explicit tile size (used on decode,
/// where the size is read from the stream).
fn for_each_tile_row_with(
    tile: u32,
    width: u32,
    height: u32,
    tx: u32,
    ty: u32,
    mut f: impl FnMut(usize, usize),
) {
    let x0 = tx * tile;
    let y0 = ty * tile;
    let x1 = (x0 + tile).min(width);
    let y1 = (y0 + tile).min(height);
    let len = (x1 - x0) as usize;
    for y in y0..y1 {
        let start = (y * width + x0) as usize;
        f(start, len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        rgba.repeat((width * height) as usize)
    }

    fn round_trip(prev: &[u8], cur: &[u8], w: u32, h: u32) -> (usize, usize) {
        let (stream, dirty, total) = encode(prev, cur, w, h);
        let back = decode(&stream, prev, w, h).expect("decode");
        assert_eq!(back, cur);
        (dirty, total)
    }

    #[test]
    fn identical_frames_have_no_dirty_tiles() {
        let (w, h) = (64, 48);
        let frame = solid(w, h, [10, 20, 30, 255]);
        let (dirty, total) = round_trip(&frame, &frame, w, h);
        assert_eq!(dirty, 0);
        assert!(total > 0);
    }

    #[test]
    fn single_changed_pixel_marks_one_tile() {
        let (w, h) = (64, 64); // 4x4 = 16 tiles of 16px
        let prev = solid(w, h, [0, 0, 0, 255]);
        let mut cur = prev.clone();
        // Change a pixel in the top-left tile.
        cur[0] = 255;
        let (dirty, total) = round_trip(&prev, &cur, w, h);
        assert_eq!(dirty, 1);
        assert_eq!(total, 16);
    }

    #[test]
    fn full_change_marks_all_tiles() {
        let (w, h) = (33, 17); // deliberately not tile-aligned
        let prev = solid(w, h, [1, 1, 1, 255]);
        let cur = solid(w, h, [2, 3, 4, 255]);
        let (dirty, total) = round_trip(&prev, &cur, w, h);
        assert_eq!(dirty, total);
    }

    #[test]
    fn non_aligned_dimensions_round_trip() {
        let (w, h) = (50, 30);
        let mut state = 0xABCD_1234u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        let prev: Vec<u8> = (0..(w * h * 4)).map(|_| next()).collect();
        let cur: Vec<u8> = (0..(w * h * 4)).map(|_| next()).collect();
        round_trip(&prev, &cur, w, h);
    }

    #[test]
    fn rejects_truncated_delta() {
        let (w, h) = (32, 32);
        let prev = solid(w, h, [0, 0, 0, 255]);
        let mut cur = prev.clone();
        cur[0] = 9;
        let (mut stream, _, _) = encode(&prev, &cur, w, h);
        stream.pop(); // drop a delta byte
        assert!(decode(&stream, &prev, w, h).is_none());
    }
}
