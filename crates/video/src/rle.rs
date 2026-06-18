//! Byte-oriented run-length coding (PackBits-style) — owned and dependency-free.
//!
//! One stage of [`crate::pack`]. It collapses runs of identical bytes, which is
//! exactly what inter-frame deltas produce (unchanged pixels delta to `0`).
//!
//! Each packet starts with a header byte interpreted as `i8`:
//! - `0..=127`   → copy the next `header + 1` literal bytes.
//! - `-1..=-127` → repeat the next single byte `1 - header` times (2..=128).
//! - `-128`      → reserved no-op (never emitted; skipped on decode).

/// Longest run or literal span a single packet can hold.
const MAX_RUN: usize = 128;

/// Run-length encode `data`.
pub(crate) fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let n = data.len();
    let mut i = 0;
    while i < n {
        // How many identical bytes start at `i` (capped to a single packet).
        let mut run = 1usize;
        while i + run < n && data[i + run] == data[i] && run < MAX_RUN {
            run += 1;
        }
        if run >= 3 {
            // Repeat packet: header = (1 - run) as i8, stored as u8.
            out.push((257 - run) as u8);
            out.push(data[i]);
            i += run;
        } else {
            // Literal packet: gather bytes until a run of >= 3 appears.
            let start = i;
            let mut count = 0usize;
            while i < n && count < MAX_RUN {
                let mut look = 1usize;
                while i + look < n && data[i + look] == data[i] && look < 3 {
                    look += 1;
                }
                if look >= 3 {
                    break;
                }
                i += 1;
                count += 1;
            }
            out.push((count - 1) as u8);
            out.extend_from_slice(&data[start..start + count]);
        }
    }
    out
}

/// Decode a buffer produced by [`encode`]. Returns `None` if it is truncated.
pub(crate) fn decode(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let n = data.len();
    let mut i = 0;
    while i < n {
        let header = data[i] as i8;
        i += 1;
        if header >= 0 {
            let count = header as usize + 1;
            let end = i.checked_add(count)?;
            if end > n {
                return None;
            }
            out.extend_from_slice(&data[i..end]);
            i = end;
        } else if header != -128 {
            let count = (1 - header as i32) as usize;
            let byte = *data.get(i)?;
            i += 1;
            out.resize(out.len() + count, byte);
        }
        // header == -128 is a no-op.
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(data: &[u8]) {
        let encoded = encode(data);
        let back = decode(&encoded).expect("decode");
        assert_eq!(back, data);
    }

    #[test]
    fn empty() {
        round_trip(&[]);
    }

    #[test]
    fn single_byte() {
        round_trip(&[9]);
    }

    #[test]
    fn pure_run() {
        round_trip(&[5u8; 1000]);
    }

    #[test]
    fn pure_literals() {
        let data: Vec<u8> = (0..200).map(|i| (i % 251) as u8).collect();
        round_trip(&data);
    }

    #[test]
    fn mixed_runs_and_literals() {
        let mut data = Vec::new();
        data.extend_from_slice(&[1, 2, 3, 4]);
        data.extend_from_slice(&[7u8; 300]);
        data.extend_from_slice(&[8, 9]);
        data.extend_from_slice(&[0u8; 130]);
        round_trip(&data);
    }

    #[test]
    fn two_byte_run_stays_literal() {
        // Runs shorter than 3 are encoded as literals, and still round-trip.
        round_trip(&[1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn compresses_long_runs() {
        let data = vec![0u8; 10_000];
        assert!(encode(&data).len() < data.len());
    }
}
