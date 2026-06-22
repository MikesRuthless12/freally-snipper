//! Random-access `.fvid` reading for the timeline editor (P6.1, B5/B6).
//!
//! The owned [`StreamDecoder`] is **forward-only** (no seek): it yields frames
//! 0,1,2,… in order. The timeline needs an arbitrary frame *now* (scrub, JKL,
//! frame-step), so [`FvidReader`] wraps the decoder with:
//!
//! - a **forward cursor** — kept positioned so sequential reads (playback,
//!   forward scrub) just pull the next frame, no re-open;
//! - **decode-to-N** — a backward jump re-opens the stream and decodes forward to
//!   the target (the codec keyframes every 120 frames, but exposes no index, so
//!   for the foundation we re-decode from the start; an explicit keyframe index is
//!   a later perf pass);
//! - an **LRU frame cache** so re-visited frames are instant and memory stays
//!   bounded on long/4K clips.
//!
//! Audio is small relative to video (48 kHz stereo i16 ≈ 192 KB/s), so the whole
//! track is decoded once on open and sliced for the mixer.
//!
//! [`AppProvider`] implements [`MediaProvider`] over a set of readers, so the
//! pure [`freally_timeline`] compositor can pull real media.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use freally_timeline::{MediaId, MediaProvider};
use freally_video::{read_audio_file, AudioTrack, Frame, Rational, StreamDecoder};

/// Default decoded-frame cache size (frames). 256 × 1080p RGBA ≈ 2 GB worst case,
/// but real clips are far smaller and the LRU evicts as it fills.
const DEFAULT_CACHE_FRAMES: usize = 256;

/// A seekable view over one `.fvid` file: random-access frames + the decoded audio.
pub struct FvidReader {
    path: PathBuf,
    width: u32,
    height: u32,
    fps: Rational,
    frame_count: u32,
    audio: Option<AudioTrack>,
    /// Forward decoder and the index of the **next** frame it will yield.
    cursor: Option<(StreamDecoder<std::io::BufReader<std::fs::File>>, u64)>,
    cache: FrameCache,
}

impl FvidReader {
    /// Open `path`, reading its header + (once) its audio track.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let dec = StreamDecoder::open(&path).map_err(|e| e.to_string())?;
        let (width, height, fps, frame_count) =
            (dec.width(), dec.height(), dec.fps(), dec.frame_count());
        drop(dec);
        let audio = read_audio_file(&path).map_err(|e| e.to_string())?;
        Ok(Self {
            path,
            width,
            height,
            fps,
            frame_count,
            audio,
            cursor: None,
            cache: FrameCache::new(DEFAULT_CACHE_FRAMES),
        })
    }

    /// Frame width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Source frame rate.
    pub fn fps(&self) -> Rational {
        self.fps
    }

    /// Total frame count declared in the header.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// The decoded RGBA frame at index `n`, or `None` if `n` is out of range or the
    /// file can't be read. Cached; sequential reads reuse the forward cursor.
    pub fn frame(&mut self, n: u64) -> Option<Arc<Frame>> {
        if n as u32 >= self.frame_count {
            return None;
        }
        if let Some(f) = self.cache.get(n) {
            return Some(f);
        }
        // Re-open if there's no cursor or the target is behind it (backward jump).
        let needs_reopen = match &self.cursor {
            Some((_, next)) => n < *next,
            None => true,
        };
        if needs_reopen {
            match StreamDecoder::open(&self.path) {
                Ok(dec) => self.cursor = Some((dec, 0)),
                Err(_) => return None,
            }
        }
        let (dec, next) = self.cursor.as_mut()?;
        // Decode forward, caching each frame, until we reach n.
        while *next <= n {
            match dec.next_frame() {
                Ok(Some(frame)) => {
                    let idx = *next;
                    *next += 1;
                    self.cache.put(idx, Arc::new(frame));
                }
                _ => return None, // unexpected EOF / decode error
            }
        }
        self.cache.get(n)
    }

    /// Interleaved samples for the per-channel range `[start, start+len)` at
    /// `channels`, zero-padded past the track (or when there is no audio).
    pub fn audio_slice(&self, start: u64, len: usize, channels: u16) -> Vec<i16> {
        let out_ch = channels.max(1) as usize;
        let mut out = vec![0i16; len * out_ch];
        let Some(track) = &self.audio else {
            return out;
        };
        let src_ch = track.channels.max(1) as usize;
        let total = track.frames_per_channel() as u64;
        for i in 0..len as u64 {
            let pos = start + i;
            if pos >= total {
                break;
            }
            let src = &track.samples[pos as usize * src_ch..][..src_ch];
            remix(src, &mut out[i as usize * out_ch..][..out_ch]);
        }
        out
    }
}

/// Map one interleaved sample-frame between channel counts (== passthrough, mono→
/// stereo duplicate, stereo→mono average, else copy-overlap-zero-rest).
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

/// A small capacity-bounded LRU cache of decoded frames.
struct FrameCache {
    cap: usize,
    map: HashMap<u64, Arc<Frame>>,
    order: VecDeque<u64>, // least-recently-used at the front
}

impl FrameCache {
    fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn touch(&mut self, k: u64) {
        if let Some(pos) = self.order.iter().position(|&x| x == k) {
            self.order.remove(pos);
        }
        self.order.push_back(k);
    }

    fn get(&mut self, k: u64) -> Option<Arc<Frame>> {
        let v = self.map.get(&k).cloned()?;
        self.touch(k);
        Some(v)
    }

    fn put(&mut self, k: u64, v: Arc<Frame>) {
        if self.map.insert(k, v).is_none() {
            // New key: evict the LRU entry if we're over capacity.
            while self.order.len() >= self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                } else {
                    break;
                }
            }
        }
        self.touch(k);
    }
}

/// A [`MediaProvider`] over a set of open [`FvidReader`]s, keyed by [`MediaId`].
#[derive(Default)]
pub struct AppProvider {
    readers: HashMap<MediaId, FvidReader>,
}

impl AppProvider {
    /// An empty provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an open reader under `id`.
    pub fn insert(&mut self, id: MediaId, reader: FvidReader) {
        self.readers.insert(id, reader);
    }
}

impl MediaProvider for AppProvider {
    fn frame(&mut self, id: MediaId, src_index: u64) -> Option<Frame> {
        self.readers
            .get_mut(&id)
            .and_then(|r| r.frame(src_index))
            .map(|a| (*a).clone())
    }

    fn samples(&mut self, id: MediaId, start_sample: u64, len: usize, channels: u16) -> Vec<i16> {
        match self.readers.get_mut(&id) {
            Some(r) => r.audio_slice(start_sample, len, channels),
            None => vec![0; len * channels.max(1) as usize],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freally_video::StreamEncoder;
    use std::path::Path;

    fn solid(w: u32, h: u32, v: u8) -> Frame {
        Frame::from_rgba(w, h, &[v, v, v, 255].repeat((w * h) as usize)).unwrap()
    }

    /// Write a `count`-frame clip whose every frame is a unique grey level, so a
    /// frame's value identifies its index.
    fn write_ramp(path: &Path, w: u32, h: u32, count: u32) {
        let mut enc = StreamEncoder::create(path, w, h, Rational::new(30, 1)).unwrap();
        for i in 0..count {
            enc.push_frame(&solid(w, h, i as u8)).unwrap();
        }
        enc.finish().unwrap();
    }

    #[test]
    fn random_access_returns_the_right_frames() {
        let path = std::env::temp_dir().join(format!("fvid_reader_ra_{}.fvid", std::process::id()));
        write_ramp(&path, 8, 8, 200);
        let mut r = FvidReader::open(&path).unwrap();
        assert_eq!(r.frame_count(), 200);

        // Forward, backward, and repeated reads all land on the right frame
        // (frame i is filled with grey level (i % 256)).
        for &i in &[0u64, 5, 130, 42, 199, 0, 150, 7] {
            let f = r.frame(i).expect("frame in range");
            assert_eq!(f.pixels[0], (i % 256) as u8, "frame {i}");
        }
        assert!(r.frame(200).is_none()); // out of range
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lru_cache_evicts_but_stays_correct() {
        let mut cache = FrameCache::new(2);
        cache.put(1, Arc::new(solid(1, 1, 1)));
        cache.put(2, Arc::new(solid(1, 1, 2)));
        assert!(cache.get(1).is_some()); // 1 now most-recent
        cache.put(3, Arc::new(solid(1, 1, 3))); // evicts 2 (LRU), keeps 1 and 3
        assert!(cache.get(2).is_none());
        assert!(cache.get(1).is_some());
        assert!(cache.get(3).is_some());
    }

    #[test]
    fn audio_slice_zero_pads_and_remixes() {
        // No audio → silence of the right length.
        let path = std::env::temp_dir().join(format!("fvid_reader_au_{}.fvid", std::process::id()));
        write_ramp(&path, 4, 4, 3);
        let r = FvidReader::open(&path).unwrap();
        // A video-only file yields silence of the requested length.
        assert_eq!(r.audio_slice(0, 4, 2), vec![0; 8]);
        let _ = std::fs::remove_file(&path);
    }
}
