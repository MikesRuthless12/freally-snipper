//! Media resolution: the compositor pulls source pixels and samples through a
//! [`MediaProvider`], keeping the [`Timeline`](crate::Timeline) model pure (no
//! I/O, no decoders). The app implements this over the random-access `.fvid`
//! reader; tests and small in-RAM media use [`MemoryProvider`].
//!
//! **Format contract:** a provider returns media already conformed to the
//! project — frames at the timeline's size/`fps`, audio at its `sample_rate` and
//! requested `channels`. (Import-time conform is the app's job.)

use std::collections::HashMap;

use freally_video::{Frame, Movie};

use crate::MediaId;

/// Resolves a [`MediaId`] to decoded frames and audio samples on demand.
pub trait MediaProvider {
    /// The source's RGBA frame at source-frame index `src_index`, or `None` if the
    /// id is unknown or the index is out of range.
    fn frame(&mut self, id: MediaId, src_index: u64) -> Option<Frame>;

    /// Interleaved samples for the per-channel range `[start_sample, +len)` at the
    /// project `channels`. Positions past the source (or unknown ids) are silence,
    /// so the returned buffer is always exactly `len * channels` long.
    fn samples(&mut self, id: MediaId, start_sample: u64, len: usize, channels: u16) -> Vec<i16>;
}

/// An in-memory [`MediaProvider`] backed by fully-decoded [`Movie`]s — used by the
/// crate's tests and any caller whose media is small enough to hold in RAM.
#[derive(Default)]
pub struct MemoryProvider {
    media: HashMap<MediaId, Movie>,
}

impl MemoryProvider {
    /// An empty provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `movie` under `id` (replacing any previous entry).
    pub fn insert(&mut self, id: MediaId, movie: Movie) {
        self.media.insert(id, movie);
    }
}

impl MediaProvider for MemoryProvider {
    fn frame(&mut self, id: MediaId, src_index: u64) -> Option<Frame> {
        self.media
            .get(&id)
            .and_then(|m| m.frames.get(src_index as usize).cloned())
    }

    fn samples(&mut self, id: MediaId, start_sample: u64, len: usize, channels: u16) -> Vec<i16> {
        let out_ch = channels.max(1) as usize;
        let mut out = vec![0i16; len * out_ch];
        let Some(track) = self.media.get(&id).and_then(|m| m.audio.as_ref()) else {
            return out;
        };
        let src_ch = track.channels.max(1) as usize;
        let total = track.frames_per_channel() as u64;
        for i in 0..len as u64 {
            let pos = start_sample + i;
            if pos >= total {
                break; // silence past the end
            }
            let src = &track.samples[pos as usize * src_ch..][..src_ch];
            let dst = &mut out[i as usize * out_ch..][..out_ch];
            remix(src, dst);
        }
        out
    }
}

/// Map one interleaved sample-frame `src` (its own channel count) into `dst` (the
/// project channel count): pass-through when equal, duplicate mono→stereo, average
/// stereo→mono, else copy the overlap and zero the rest.
fn remix(src: &[i16], dst: &mut [i16]) {
    match (src.len(), dst.len()) {
        (s, d) if s == d => dst.copy_from_slice(src),
        (1, _) => dst.fill(src[0]),
        (_, 1) => {
            let sum: i32 = src.iter().map(|&v| v as i32).sum();
            dst[0] = (sum / src.len() as i32) as i16;
        }
        _ => {
            let n = src.len().min(dst.len());
            dst[..n].copy_from_slice(&src[..n]);
            dst[n..].fill(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freally_video::{AudioTrack, Rational};

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Frame {
        Frame::from_rgba(w, h, &rgba.repeat((w * h) as usize)).unwrap()
    }

    #[test]
    fn unknown_id_is_silence_and_no_frame() {
        let mut p = MemoryProvider::new();
        assert!(p.frame(MediaId(9), 0).is_none());
        assert_eq!(p.samples(MediaId(9), 0, 4, 2), vec![0; 8]);
    }

    #[test]
    fn frames_are_returned_in_range() {
        let mut p = MemoryProvider::new();
        let frames = vec![solid(2, 2, [1, 2, 3, 255]), solid(2, 2, [9, 9, 9, 255])];
        p.insert(
            MediaId(1),
            Movie::new(2, 2, Rational::new(30, 1), frames, None),
        );
        assert_eq!(p.frame(MediaId(1), 0).unwrap().pixels[0], 1);
        assert_eq!(p.frame(MediaId(1), 1).unwrap().pixels[0], 9);
        assert!(p.frame(MediaId(1), 2).is_none());
    }

    #[test]
    fn samples_slice_with_zero_pad_past_end() {
        let mut p = MemoryProvider::new();
        // 3 stereo sample-frames: (1,2) (3,4) (5,6).
        let audio = AudioTrack::new(48_000, 2, vec![1, 2, 3, 4, 5, 6]);
        p.insert(
            MediaId(1),
            Movie::new(0, 0, Rational::new(30, 1), vec![], Some(audio)),
        );
        // Ask for 4 sample-frames starting at 1 → (3,4) (5,6) (0,0) (0,0).
        assert_eq!(p.samples(MediaId(1), 1, 4, 2), vec![3, 4, 5, 6, 0, 0, 0, 0]);
    }

    #[test]
    fn mono_source_upmixes_to_stereo() {
        let mut p = MemoryProvider::new();
        let audio = AudioTrack::new(48_000, 1, vec![7, 8, 9]);
        p.insert(
            MediaId(1),
            Movie::new(0, 0, Rational::new(30, 1), vec![], Some(audio)),
        );
        assert_eq!(p.samples(MediaId(1), 0, 3, 2), vec![7, 7, 8, 8, 9, 9]);
    }

    #[test]
    fn stereo_source_downmixes_to_mono() {
        let mut p = MemoryProvider::new();
        let audio = AudioTrack::new(48_000, 2, vec![10, 20, 30, 50]);
        p.insert(
            MediaId(1),
            Movie::new(0, 0, Rational::new(30, 1), vec![], Some(audio)),
        );
        assert_eq!(p.samples(MediaId(1), 0, 2, 1), vec![15, 40]);
    }
}
