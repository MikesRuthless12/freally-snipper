//! `freally-video` — the OWNED video codec for Freally Snipper.
//!
//! `freally-video` is built **only** from expired-patent / public-domain
//! techniques and has **zero third-party dependencies** — it is 100% owned and
//! patent-safe, and is the default record / project format. This crate is the
//! Phase 5 codec foundation (P5.0); recording (P5.1+) and the Phase 6 editor
//! build on top of it.
//!
//! ## What's here
//!
//! - [`Movie`] — a decoded video: timing ([`Rational`] fps), a list of RGBA
//!   [`Frame`]s, and an optional PCM [`AudioTrack`].
//! - [`Movie::encode`] / [`Movie::decode`] — round-trip a `Movie` to and from
//!   the `.fvid` container bytes (and [`Movie::write_file`] / [`Movie::read_file`]).
//!
//! ## How it works (all owned)
//!
//! - **Intra frames** (keyframes) use the Freally intra codec — a from-scratch,
//!   lossless QOI-class RGBA coder ([`mod@intra`]).
//! - **Inter frames** store only the tiles that changed, as wrapping per-byte
//!   deltas from the previous frame ([`mod@inter`]).
//! - Every payload is then entropy-coded with an owned RLE + canonical-Huffman
//!   stage, keeping whichever is smallest ([`mod@pack`]).
//! - The `.fvid` container ([`Movie::encode`]) frames it all with a small header.
//!
//! The whole pipeline is **lossless**: decoding reproduces the exact RGBA bytes
//! and PCM samples that were encoded.
//!
//! ```
//! use freally_video::{Frame, Movie, Rational};
//!
//! let (w, h) = (2u32, 2u32);
//! let red = Frame::from_rgba(w, h, &[255, 0, 0, 255].repeat(4)).unwrap();
//! let movie = Movie::new(w, h, Rational::new(30, 1), vec![red], None);
//!
//! let bytes = movie.encode().unwrap();
//! let decoded = Movie::decode(&bytes).unwrap();
//! assert_eq!(decoded.frames[0].pixels, movie.frames[0].pixels);
//! ```
#![forbid(unsafe_code)]

mod bitio;
mod bytes;
mod huffman;
mod inter;
mod intra;
mod pack;
mod rle;

use std::path::Path;

use bytes::{put_u16, put_u32, put_u64, Cursor};

/// Identifier for this crate, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-video";

/// Magic bytes at the start of every `.fvid` stream.
pub const MAGIC: &[u8; 4] = b"FVID";

/// On-disk container format version written by this build.
pub const FORMAT_VERSION: u16 = 1;

/// Canonical file extension for the owned container.
pub const FILE_EXTENSION: &str = "fvid";

// Header flag bits.
const FLAG_AUDIO: u16 = 1 << 0;

// Audio sample format tags.
const AUDIO_FORMAT_S16LE: u8 = 1;

// Per-frame type tags.
const FRAME_INTRA: u8 = 0;
const FRAME_INTER: u8 = 1;

/// Force a keyframe at least this often, so playback can resync and (later)
/// seeking has anchor points.
const KEYFRAME_INTERVAL: u32 = 120;

/// If at least this fraction of a frame's tiles changed, encode it as a
/// keyframe instead of a delta (a scene cut compresses better intra).
const SCENE_CHANGE_RATIO: f32 = 0.9;

/// A frame rate expressed as an exact rational (`num / den`), e.g. `30/1` or
/// `30000/1001`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rational {
    pub num: u32,
    pub den: u32,
}

impl Rational {
    /// Construct a rational. A zero denominator is rejected later by
    /// [`Movie::encode`].
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

    /// The value as `f64` (returns `0.0` if the denominator is zero).
    pub fn as_f64(self) -> f64 {
        if self.den == 0 {
            0.0
        } else {
            f64::from(self.num) / f64::from(self.den)
        }
    }
}

/// A single video frame: tightly-packed RGBA8 pixels (`width * height * 4`
/// bytes, row-major, no padding).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Frame {
    /// Build a frame from an owned RGBA8 buffer, validating its length.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        if pixels.len() != rgba_len(width, height)? {
            return Err(VideoError::InvalidData(
                "pixel buffer length != width*height*4",
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Build a frame by copying an RGBA8 slice, validating its length.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Self> {
        Self::new(width, height, rgba.to_vec())
    }
}

/// A simple interleaved 16-bit PCM audio track (the owned default audio scheme).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioTrack {
    /// Samples per second per channel (e.g. `48_000`).
    pub sample_rate: u32,
    /// Number of interleaved channels (e.g. `2` for stereo).
    pub channels: u16,
    /// Interleaved signed 16-bit samples (`len` is a multiple of `channels`).
    pub samples: Vec<i16>,
}

impl AudioTrack {
    /// Construct an audio track. Invariants are checked by [`Movie::encode`].
    pub fn new(sample_rate: u32, channels: u16, samples: Vec<i16>) -> Self {
        Self {
            sample_rate,
            channels,
            samples,
        }
    }

    /// Number of samples per channel (`0` if the track has no channels).
    pub fn frames_per_channel(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }
}

/// A decoded video: timing, frames, and optional audio. Encodes to / decodes
/// from the owned `.fvid` container.
#[derive(Clone, Debug)]
pub struct Movie {
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub frames: Vec<Frame>,
    pub audio: Option<AudioTrack>,
}

impl Movie {
    /// Assemble a movie from its parts. Call [`Movie::encode`] to serialize
    /// (which validates frame sizes, audio layout, and the frame rate).
    pub fn new(
        width: u32,
        height: u32,
        fps: Rational,
        frames: Vec<Frame>,
        audio: Option<AudioTrack>,
    ) -> Self {
        Self {
            width,
            height,
            fps,
            frames,
            audio,
        }
    }

    /// Serialize to the `.fvid` container bytes (lossless).
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;

        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u16(&mut out, FORMAT_VERSION);
        put_u16(&mut out, if self.audio.is_some() { FLAG_AUDIO } else { 0 });
        put_u32(&mut out, self.width);
        put_u32(&mut out, self.height);
        put_u32(&mut out, self.fps.num);
        put_u32(&mut out, self.fps.den);
        put_u32(&mut out, self.frames.len() as u32);
        match &self.audio {
            Some(a) => {
                put_u32(&mut out, a.sample_rate);
                put_u16(&mut out, a.channels);
                out.push(AUDIO_FORMAT_S16LE);
                put_u64(&mut out, a.samples.len() as u64);
            }
            None => {
                put_u32(&mut out, 0);
                put_u16(&mut out, 0);
                out.push(0);
                put_u64(&mut out, 0);
            }
        }

        let mut prev: Option<&[u8]> = None;
        for (i, frame) in self.frames.iter().enumerate() {
            let mut wrote_inter = false;
            if let Some(prev_pixels) = prev {
                if !(i as u32).is_multiple_of(KEYFRAME_INTERVAL) {
                    let (delta, dirty, total) = inter::encode(
                        prev_pixels,
                        frame.pixels.as_slice(),
                        self.width,
                        self.height,
                    );
                    let scene_change =
                        total > 0 && dirty as f32 >= SCENE_CHANGE_RATIO * total as f32;
                    if !scene_change {
                        write_block(&mut out, FRAME_INTER, &pack::pack(&delta))?;
                        wrote_inter = true;
                    }
                }
            }
            if !wrote_inter {
                let block = pack::pack(&intra::encode(frame.pixels.as_slice()));
                write_block(&mut out, FRAME_INTRA, &block)?;
            }
            prev = Some(frame.pixels.as_slice());
        }

        if let Some(a) = &self.audio {
            let block = pack::pack(&samples_to_bytes(&a.samples));
            let len = u32::try_from(block.len())
                .map_err(|_| VideoError::InvalidData("audio block too large"))?;
            put_u32(&mut out, len);
            out.extend_from_slice(&block);
        }
        Ok(out)
    }

    /// Parse `.fvid` container bytes back into a [`Movie`] (lossless).
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        if cur.take(4)? != MAGIC.as_slice() {
            return Err(VideoError::BadMagic);
        }
        let version = cur.read_u16()?;
        if version != FORMAT_VERSION {
            return Err(VideoError::UnsupportedVersion(version));
        }
        let flags = cur.read_u16()?;
        let width = cur.read_u32()?;
        let height = cur.read_u32()?;
        let fps = Rational::new(cur.read_u32()?, cur.read_u32()?);
        let frame_count = cur.read_u32()?;
        let audio_rate = cur.read_u32()?;
        let audio_channels = cur.read_u16()?;
        let audio_format = cur.read_u8()?;
        let audio_samples = cur.read_u64()?;

        let frame_bytes = rgba_len(width, height)?;
        let pixel_count = frame_bytes / 4;

        let mut frames = Vec::with_capacity(frame_count as usize);
        let mut prev: Option<Vec<u8>> = None;
        for _ in 0..frame_count {
            let ftype = cur.read_u8()?;
            let len = cur.read_u32()? as usize;
            let block = cur.take(len)?;
            let payload = pack::unpack(block).ok_or(VideoError::InvalidData("corrupt block"))?;
            let pixels = match ftype {
                FRAME_INTRA => {
                    let px = intra::decode(&payload, pixel_count)
                        .ok_or(VideoError::InvalidData("corrupt intra frame"))?;
                    if px.len() != frame_bytes {
                        return Err(VideoError::InvalidData("intra frame has wrong size"));
                    }
                    px
                }
                FRAME_INTER => {
                    let prev_pixels = prev
                        .as_deref()
                        .ok_or(VideoError::InvalidData("inter frame before any keyframe"))?;
                    inter::decode(&payload, prev_pixels, width, height)
                        .ok_or(VideoError::InvalidData("corrupt inter frame"))?
                }
                _ => return Err(VideoError::InvalidData("unknown frame type")),
            };
            frames.push(Frame {
                width,
                height,
                pixels: pixels.clone(),
            });
            prev = Some(pixels);
        }

        let audio = if flags & FLAG_AUDIO != 0 {
            if audio_format != AUDIO_FORMAT_S16LE {
                return Err(VideoError::InvalidData("unsupported audio format"));
            }
            if audio_channels == 0 {
                return Err(VideoError::InvalidData("audio track has zero channels"));
            }
            let len = cur.read_u32()? as usize;
            let block = cur.take(len)?;
            let pcm = pack::unpack(block).ok_or(VideoError::InvalidData("corrupt audio block"))?;
            if !pcm.len().is_multiple_of(2) {
                return Err(VideoError::InvalidData("audio byte count is odd"));
            }
            let samples = bytes_to_samples(&pcm);
            if samples.len() as u64 != audio_samples {
                return Err(VideoError::InvalidData("audio sample count mismatch"));
            }
            Some(AudioTrack {
                sample_rate: audio_rate,
                channels: audio_channels,
                samples,
            })
        } else {
            None
        };

        Ok(Self {
            width,
            height,
            fps,
            frames,
            audio,
        })
    }

    /// Encode and write the movie to a `.fvid` file.
    pub fn write_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let bytes = self.encode()?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Read and decode a movie from a `.fvid` file.
    pub fn read_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::decode(&bytes)
    }

    /// Validate the invariants the container relies on.
    fn validate(&self) -> Result<()> {
        if self.fps.den == 0 {
            return Err(VideoError::InvalidData("fps denominator is zero"));
        }
        if self.frames.len() > u32::MAX as usize {
            return Err(VideoError::InvalidData("too many frames"));
        }
        let frame_bytes = rgba_len(self.width, self.height)?;
        for f in &self.frames {
            if f.width != self.width || f.height != self.height {
                return Err(VideoError::DimensionMismatch);
            }
            if f.pixels.len() != frame_bytes {
                return Err(VideoError::InvalidData(
                    "frame pixel buffer has wrong length",
                ));
            }
        }
        if let Some(a) = &self.audio {
            if a.channels == 0 {
                return Err(VideoError::InvalidData("audio track has zero channels"));
            }
            if !a.samples.len().is_multiple_of(a.channels as usize) {
                return Err(VideoError::InvalidData(
                    "audio samples not a multiple of channels",
                ));
            }
        }
        Ok(())
    }
}

/// Append a `[type][u32 len][block]` record, checking the length fits a `u32`.
fn write_block(out: &mut Vec<u8>, frame_type: u8, block: &[u8]) -> Result<()> {
    let len =
        u32::try_from(block.len()).map_err(|_| VideoError::InvalidData("frame block too large"))?;
    out.push(frame_type);
    put_u32(out, len);
    out.extend_from_slice(block);
    Ok(())
}

/// `width * height * 4`, with overflow rejected.
fn rgba_len(width: u32, height: u32) -> Result<usize> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(VideoError::InvalidData("frame dimensions overflow"))
}

fn samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn bytes_to_samples(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Errors produced when encoding or decoding `freally-video` data.
#[derive(Debug)]
pub enum VideoError {
    /// The byte stream ended before a complete structure could be read.
    Truncated,
    /// The stream did not start with the [`MAGIC`] bytes.
    BadMagic,
    /// The stream's format version is not understood by this build.
    UnsupportedVersion(u16),
    /// The stream was well-formed enough to parse but internally invalid.
    InvalidData(&'static str),
    /// A frame's dimensions did not match the movie's dimensions.
    DimensionMismatch,
    /// An I/O error from [`Movie::read_file`] / [`Movie::write_file`].
    Io(std::io::Error),
}

impl std::fmt::Display for VideoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(f, "freally-video stream is truncated"),
            Self::BadMagic => write!(f, "not a freally-video (.fvid) stream"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported .fvid format version {v}"),
            Self::InvalidData(why) => write!(f, "invalid freally-video data: {why}"),
            Self::DimensionMismatch => write!(f, "frame dimensions do not match the movie"),
            Self::Io(e) => write!(f, "freally-video I/O error: {e}"),
        }
    }
}

impl std::error::Error for VideoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for VideoError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Result type for this crate.
pub type Result<T> = std::result::Result<T, VideoError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap deterministic PRNG so tests need no external crate.
    struct Lcg(u32);
    impl Lcg {
        fn next_u8(&mut self) -> u8 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 24) as u8
        }
    }

    fn solid_frame(w: u32, h: u32, rgba: [u8; 4]) -> Frame {
        Frame::from_rgba(w, h, &rgba.repeat((w * h) as usize)).unwrap()
    }

    fn assert_movies_equal(a: &Movie, b: &Movie) {
        assert_eq!(a.width, b.width);
        assert_eq!(a.height, b.height);
        assert_eq!(a.fps, b.fps);
        assert_eq!(a.frames, b.frames);
        assert_eq!(a.audio, b.audio);
    }

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "freally-video");
    }

    #[test]
    fn video_only_round_trip_with_motion() {
        let (w, h) = (48, 32);
        let mut frames = vec![solid_frame(w, h, [20, 40, 60, 255])];
        // A moving 4x4 white block on each subsequent frame.
        for step in 0..5u32 {
            let mut px = [20u8, 40, 60, 255].repeat((w * h) as usize);
            for dy in 0..4 {
                for dx in 0..4 {
                    let x = step + dx;
                    let y = 1 + dy;
                    let idx = ((y * w + x) * 4) as usize;
                    px[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
            frames.push(Frame::new(w, h, px).unwrap());
        }
        let movie = Movie::new(w, h, Rational::new(30, 1), frames, None);
        let bytes = movie.encode().unwrap();
        assert_eq!(&bytes[0..4], MAGIC.as_slice());
        let decoded = Movie::decode(&bytes).unwrap();
        assert_movies_equal(&movie, &decoded);
    }

    #[test]
    fn audio_only_round_trip() {
        let samples: Vec<i16> = (0..4800).map(|i| ((i * 37) % 1000 - 500) as i16).collect();
        let audio = AudioTrack::new(48_000, 2, samples);
        let movie = Movie::new(0, 0, Rational::new(30, 1), Vec::new(), Some(audio));
        let bytes = movie.encode().unwrap();
        let decoded = Movie::decode(&bytes).unwrap();
        assert_movies_equal(&movie, &decoded);
    }

    #[test]
    fn audio_and_video_round_trip() {
        let (w, h) = (16, 16);
        let frames = vec![
            solid_frame(w, h, [0, 0, 0, 255]),
            solid_frame(w, h, [255, 0, 0, 255]),
            solid_frame(w, h, [0, 255, 0, 255]),
        ];
        let samples: Vec<i16> = (0..2400).map(|i| (i % 256 - 128) as i16).collect();
        let movie = Movie::new(
            w,
            h,
            Rational::new(30000, 1001),
            frames,
            Some(AudioTrack::new(44_100, 1, samples)),
        );
        let bytes = movie.encode().unwrap();
        let decoded = Movie::decode(&bytes).unwrap();
        assert_movies_equal(&movie, &decoded);
        assert_eq!(decoded.fps.den, 1001);
    }

    #[test]
    fn keyframe_interval_is_exercised() {
        // More frames than the keyframe interval, with small per-frame motion.
        let (w, h) = (8, 8);
        let mut frames = Vec::new();
        for i in 0..(KEYFRAME_INTERVAL + 10) {
            let v = (i % 256) as u8;
            frames.push(solid_frame(w, h, [v, 0, 0, 255]));
        }
        let movie = Movie::new(w, h, Rational::new(30, 1), frames, None);
        let decoded = Movie::decode(&movie.encode().unwrap()).unwrap();
        assert_movies_equal(&movie, &decoded);
    }

    #[test]
    fn scene_change_round_trips() {
        let (w, h) = (32, 32);
        let frames = vec![
            solid_frame(w, h, [10, 10, 10, 255]),
            solid_frame(w, h, [200, 200, 200, 255]), // every tile differs
        ];
        let movie = Movie::new(w, h, Rational::new(24, 1), frames, None);
        let decoded = Movie::decode(&movie.encode().unwrap()).unwrap();
        assert_movies_equal(&movie, &decoded);
    }

    #[test]
    fn random_frames_round_trip() {
        let (w, h) = (40, 24);
        let mut rng = Lcg(0xDEAD_BEEF);
        let frames: Vec<Frame> = (0..4)
            .map(|_| {
                let px: Vec<u8> = (0..(w * h * 4)).map(|_| rng.next_u8()).collect();
                Frame::new(w, h, px).unwrap()
            })
            .collect();
        let movie = Movie::new(w, h, Rational::new(60, 1), frames, None);
        let decoded = Movie::decode(&movie.encode().unwrap()).unwrap();
        assert_movies_equal(&movie, &decoded);
    }

    #[test]
    fn static_clip_compresses_well() {
        // 60 identical frames should be far smaller than the raw pixel data.
        let (w, h) = (320, 240);
        let frames = vec![solid_frame(w, h, [12, 34, 56, 255]); 60];
        let raw = (w * h * 4 * 60) as usize;
        let movie = Movie::new(w, h, Rational::new(30, 1), frames, None);
        let encoded = movie.encode().unwrap();
        assert!(
            encoded.len() < raw / 50,
            "expected strong compression: {} vs raw {raw}",
            encoded.len()
        );
        // And it must still round-trip exactly.
        let decoded = Movie::decode(&encoded).unwrap();
        assert_movies_equal(&movie, &decoded);
    }

    #[test]
    fn file_round_trip() {
        let (w, h) = (16, 12);
        let frames = vec![
            solid_frame(w, h, [1, 2, 3, 255]),
            solid_frame(w, h, [4, 5, 6, 255]),
        ];
        let movie = Movie::new(w, h, Rational::new(30, 1), frames, None);
        let path = std::env::temp_dir().join(format!(
            "freally_video_file_round_trip_{}.fvid",
            std::process::id()
        ));
        movie.write_file(&path).unwrap();
        let decoded = Movie::read_file(&path).unwrap();
        assert_movies_equal(&movie, &decoded);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(matches!(
            Movie::decode(b"NOPE and some trailing bytes"),
            Err(VideoError::BadMagic)
        ));
    }

    #[test]
    fn rejects_truncated_stream() {
        let movie = Movie::new(
            4,
            4,
            Rational::new(30, 1),
            vec![solid_frame(4, 4, [9; 4])],
            None,
        );
        let mut bytes = movie.encode().unwrap();
        bytes.truncate(bytes.len() - 1);
        assert!(matches!(Movie::decode(&bytes), Err(VideoError::Truncated)));
    }

    #[test]
    fn encode_rejects_dimension_mismatch() {
        let bad = Frame {
            width: 5,
            height: 5,
            pixels: vec![0u8; 5 * 5 * 4],
        };
        let movie = Movie::new(4, 4, Rational::new(30, 1), vec![bad], None);
        assert!(matches!(movie.encode(), Err(VideoError::DimensionMismatch)));
    }

    #[test]
    fn encode_rejects_zero_fps_denominator() {
        let movie = Movie::new(4, 4, Rational::new(30, 0), Vec::new(), None);
        assert!(matches!(movie.encode(), Err(VideoError::InvalidData(_))));
    }

    #[test]
    fn frame_constructor_validates_length() {
        assert!(Frame::new(2, 2, vec![0u8; 10]).is_err());
        assert!(Frame::from_rgba(2, 2, &[0u8; 16]).is_ok());
    }
}
