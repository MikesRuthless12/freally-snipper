//! The Freally intra-frame image codec — a from-scratch, lossless RGBA coder.
//!
//! This is a QOI-class scheme (a public-domain *technique*, implemented here from
//! scratch — no third-party code). Each pixel is encoded as one of:
//!
//! - **RUN** — a run of 1..=62 identical pixels.
//! - **INDEX** — a reference into a rolling 64-entry table of recently seen pixels.
//! - **DIFF** — a small per-channel difference from the previous pixel.
//! - **LUMA** — a green difference plus red/blue differences relative to green.
//! - **RGB / RGBA** — the literal pixel.
//!
//! It is lossless and operates on tightly-packed RGBA8 (`width * height * 4`
//! bytes). Keyframes in `.fvid` are stored with this codec; see [`crate::inter`]
//! for delta frames.

const OP_INDEX: u8 = 0x00; // 0b00xx_xxxx
const OP_DIFF: u8 = 0x40; // 0b01xx_xxxx
const OP_LUMA: u8 = 0x80; // 0b10xx_xxxx
const OP_RUN: u8 = 0xC0; // 0b11xx_xxxx
const OP_RGB: u8 = 0xFE;
const OP_RGBA: u8 = 0xFF;
const MASK: u8 = 0xC0;

/// Hash a pixel into the 64-entry rolling index (the QOI hash).
fn hash(px: [u8; 4]) -> usize {
    (px[0] as usize * 3 + px[1] as usize * 5 + px[2] as usize * 7 + px[3] as usize * 11) & 63
}

/// Encode tightly-packed RGBA8 pixels (`width * height * 4` bytes) losslessly.
pub(crate) fn encode(pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() / 2 + 16);
    let mut index = [[0u8; 4]; 64];
    let mut prev = [0u8, 0, 0, 255];
    let mut run: u32 = 0;

    for chunk in pixels.chunks_exact(4) {
        let px = [chunk[0], chunk[1], chunk[2], chunk[3]];
        if px == prev {
            run += 1;
            if run == 62 {
                out.push(OP_RUN | (run as u8 - 1));
                run = 0;
            }
            continue;
        }
        if run > 0 {
            out.push(OP_RUN | (run as u8 - 1));
            run = 0;
        }

        let idx = hash(px);
        if index[idx] == px {
            out.push(OP_INDEX | idx as u8);
        } else {
            index[idx] = px;
            if px[3] == prev[3] {
                let dr = px[0] as i16 - prev[0] as i16;
                let dg = px[1] as i16 - prev[1] as i16;
                let db = px[2] as i16 - prev[2] as i16;
                let dr_dg = dr - dg;
                let db_dg = db - dg;
                if (-2..=1).contains(&dr) && (-2..=1).contains(&dg) && (-2..=1).contains(&db) {
                    let byte = (((dr + 2) << 4) | ((dg + 2) << 2) | (db + 2)) as u8;
                    out.push(OP_DIFF | byte);
                } else if (-32..=31).contains(&dg)
                    && (-8..=7).contains(&dr_dg)
                    && (-8..=7).contains(&db_dg)
                {
                    out.push(OP_LUMA | (dg + 32) as u8);
                    out.push((((dr_dg + 8) << 4) | (db_dg + 8)) as u8);
                } else {
                    out.push(OP_RGB);
                    out.extend_from_slice(&px[0..3]);
                }
            } else {
                out.push(OP_RGBA);
                out.extend_from_slice(&px);
            }
        }
        prev = px;
    }
    if run > 0 {
        out.push(OP_RUN | (run as u8 - 1));
    }
    out
}

/// Decode a stream from [`encode`] into `pixel_count` RGBA8 pixels
/// (`pixel_count * 4` bytes). Returns `None` if the stream is truncated.
pub(crate) fn decode(stream: &[u8], pixel_count: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(pixel_count * 4);
    let mut index = [[0u8; 4]; 64];
    let mut px = [0u8, 0, 0, 255];
    let mut i = 0;

    while out.len() < pixel_count * 4 {
        let b = *stream.get(i)?;
        i += 1;
        if b == OP_RGB {
            let bytes = stream.get(i..i + 3)?;
            px = [bytes[0], bytes[1], bytes[2], px[3]];
            i += 3;
        } else if b == OP_RGBA {
            let bytes = stream.get(i..i + 4)?;
            px = [bytes[0], bytes[1], bytes[2], bytes[3]];
            i += 4;
        } else {
            match b & MASK {
                OP_INDEX => {
                    px = index[(b & 0x3F) as usize];
                }
                OP_DIFF => {
                    let dr = ((b >> 4) & 0x03) as i16 - 2;
                    let dg = ((b >> 2) & 0x03) as i16 - 2;
                    let db = (b & 0x03) as i16 - 2;
                    px[0] = px[0].wrapping_add(dr as u8);
                    px[1] = px[1].wrapping_add(dg as u8);
                    px[2] = px[2].wrapping_add(db as u8);
                }
                OP_LUMA => {
                    let b2 = *stream.get(i)?;
                    i += 1;
                    let dg = (b & 0x3F) as i16 - 32;
                    let dr = dg + ((b2 >> 4) & 0x0F) as i16 - 8;
                    let db = dg + (b2 & 0x0F) as i16 - 8;
                    px[0] = px[0].wrapping_add(dr as u8);
                    px[1] = px[1].wrapping_add(dg as u8);
                    px[2] = px[2].wrapping_add(db as u8);
                }
                _ => {
                    // OP_RUN
                    let run = (b & 0x3F) as usize + 1;
                    for _ in 0..run {
                        if out.len() >= pixel_count * 4 {
                            break;
                        }
                        out.extend_from_slice(&px);
                    }
                    continue;
                }
            }
        }
        index[hash(px)] = px;
        out.extend_from_slice(&px);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(pixels: &[u8]) {
        let count = pixels.len() / 4;
        let stream = encode(pixels);
        let back = decode(&stream, count).expect("decode");
        assert_eq!(back, pixels);
    }

    #[test]
    fn empty() {
        round_trip(&[]);
    }

    #[test]
    fn single_pixel() {
        round_trip(&[12, 34, 56, 255]);
    }

    #[test]
    fn solid_color_uses_runs() {
        let pixels: Vec<u8> = [10, 20, 30, 255].repeat(10_000);
        round_trip(&pixels);
        // A solid frame should compress dramatically.
        assert!(encode(&pixels).len() < pixels.len() / 10);
    }

    #[test]
    fn smooth_gradient() {
        let (w, h) = (64usize, 64usize);
        let mut pixels = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                pixels.extend_from_slice(&[x as u8, y as u8, (x + y) as u8, 255]);
            }
        }
        round_trip(&pixels);
    }

    #[test]
    fn varying_alpha_uses_rgba() {
        let mut pixels = Vec::new();
        for i in 0..1000u32 {
            pixels.extend_from_slice(&[
                (i * 7) as u8,
                (i * 3) as u8,
                (i * 5) as u8,
                (i % 256) as u8,
            ]);
        }
        round_trip(&pixels);
    }

    #[test]
    fn pseudo_random_pixels() {
        let mut state = 0x90A1_B2C3u32;
        let mut pixels = Vec::with_capacity(4000);
        for _ in 0..1000 {
            for _ in 0..4 {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                pixels.push((state >> 24) as u8);
            }
        }
        round_trip(&pixels);
    }

    #[test]
    fn index_path_revisits_colors() {
        // Alternate between two colors to exercise the rolling index.
        let mut pixels = Vec::new();
        for i in 0..2000u32 {
            if i.is_multiple_of(2) {
                pixels.extend_from_slice(&[200, 100, 50, 255]);
            } else {
                pixels.extend_from_slice(&[1, 2, 3, 255]);
            }
        }
        round_trip(&pixels);
    }

    #[test]
    fn rejects_truncated_stream() {
        // Claim more pixels than the stream encodes.
        let stream = encode(&[1, 2, 3, 255]);
        assert!(decode(&stream, 1000).is_none());
    }
}
