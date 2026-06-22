//! `freally-timeline` — the pure, UI-free model + compositor for the Freally
//! Snipper video editor (Phase 6, P6.1).
//!
//! This crate owns the **edit model** (what clips sit where) and a deterministic
//! **compositor** that turns the model + decoded media into final frames and
//! mixed audio. It has no UI and (besides the owned [`freally_video`] codec for
//! its [`Frame`]/[`AudioTrack`]/[`Rational`] types) no dependencies, so the whole
//! thing is unit-testable headless — the same discipline as the codec.
//!
//! ## Shape
//!
//! - A [`Timeline`] has a size, a frame rate, an audio format, and a stack of
//!   [`Track`]s. Tracks composite **bottom-first** (later tracks draw on top).
//! - A [`Track`] holds non-overlapping [`Clip`]s (one lane of media).
//! - A [`Clip`] shows a window `[src_in, src_out)` of a [`Source`], positioned at
//!   `start` on the timeline. **Every time is in whole timeline frames**, so edits
//!   are frame-accurate by construction.
//! - The compositor ([`compose_frame`] / [`compose_audio`]) pulls source pixels and
//!   samples through a [`MediaProvider`], so the model stays pure while real media
//!   is decoded lazily (the app backs the provider with the random-access `.fvid`
//!   reader; tests use [`MemoryProvider`]).
//!
//! ## Timebase contract
//!
//! Imported media is **conformed to the project on import**: every [`Source`]
//! resolved through a provider is assumed to be at the timeline's `fps`, its audio
//! resampled to the timeline's `sample_rate` / `channels`. That lets the model use
//! a single frame timebase and derive exact sample positions for the mixer.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;

use freally_video::{Frame, Rational};

mod compositor;
mod provider;

pub use compositor::{compose_audio, compose_frame};
pub use provider::{MediaProvider, MemoryProvider};

// Re-export the codec types the model is expressed in, so callers need only depend
// on `freally-timeline`.
pub use freally_video::{AudioTrack, Frame as VideoFrame, Rational as Fps};

/// Default project audio sample rate (matches the recorder + codec: 48 kHz).
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

/// Default project channel count (stereo).
pub const DEFAULT_CHANNELS: u16 = 2;

/// Opaque handle to a media source registered with the project (a `.fvid`
/// recording, an imported video, or a still image). Resolved by a
/// [`MediaProvider`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MediaId(pub u64);

/// What a clip shows and/or plays.
#[derive(Clone, Debug)]
pub enum Source {
    /// External media resolved by a [`MediaProvider`] (a `.fvid` file, imported
    /// video, or image). Frame pixels and audio samples are fetched lazily.
    Media(MediaId),
    /// A generated solid RGBA colour — backgrounds, slates, dip-to-colour. Needs
    /// no provider and no I/O.
    Color([u8; 4]),
    /// A still RGBA image held inline, shown for the clip's whole duration (e.g. a
    /// pasted overlay or a one-frame grab). Needs no provider.
    Still(Arc<Frame>),
}

impl Source {
    /// Whether this source can contribute audio (only provider-backed media can).
    fn carries_audio(&self) -> bool {
        matches!(self, Source::Media(_))
    }
}

/// A clip placed on a track: the window `[src_in, src_out)` of a [`Source`],
/// positioned at `start` on the timeline. **All times are whole timeline frames.**
#[derive(Clone, Debug)]
pub struct Clip {
    /// Where the pixels/samples come from.
    pub source: Source,
    /// First source frame shown (inclusive). Ignored for [`Source::Color`] /
    /// [`Source::Still`] (which have no inherent frames).
    pub src_in: u64,
    /// One past the last source frame shown (exclusive); must be `> src_in`.
    pub src_out: u64,
    /// Timeline frame at which the clip starts.
    pub start: u64,
    /// Overall video opacity, `0.0..=1.0` (composited over lower tracks).
    pub opacity: f32,
    /// Linear audio gain (`1.0` = unity).
    pub gain: f32,
    /// Whether this clip's audio is silenced (it still shows video).
    pub audio_muted: bool,
    /// Cross-fade up from transparent/silent over this many frames at the head.
    pub fade_in: u64,
    /// Cross-fade down to transparent/silent over this many frames at the tail.
    pub fade_out: u64,
    /// `false` skips the clip entirely in render + mix (shown dimmed in the UI).
    pub enabled: bool,
}

impl Clip {
    /// A media clip spanning `[src_in, src_out)` of `source`, placed at `start`.
    pub fn media(id: MediaId, src_in: u64, src_out: u64, start: u64) -> Self {
        Self::new(Source::Media(id), src_in, src_out, start)
    }

    /// A solid-colour clip `len` frames long, placed at `start`.
    pub fn color(rgba: [u8; 4], len: u64, start: u64) -> Self {
        Self::new(Source::Color(rgba), 0, len.max(1), start)
    }

    /// A still-image clip `len` frames long, placed at `start`.
    pub fn still(frame: Arc<Frame>, len: u64, start: u64) -> Self {
        Self::new(Source::Still(frame), 0, len.max(1), start)
    }

    /// Build a clip with default opacity/gain (`1.0`) and no fades.
    pub fn new(source: Source, src_in: u64, src_out: u64, start: u64) -> Self {
        Self {
            source,
            src_in,
            src_out: src_out.max(src_in + 1),
            start,
            opacity: 1.0,
            gain: 1.0,
            audio_muted: false,
            fade_in: 0,
            fade_out: 0,
            enabled: true,
        }
    }

    /// Length on the timeline, in frames (`src_out - src_in`).
    pub fn duration(&self) -> u64 {
        self.src_out - self.src_in
    }

    /// One past the clip's last timeline frame (`start + duration`).
    pub fn end(&self) -> u64 {
        self.start + self.duration()
    }

    /// Whether timeline frame `t` falls within the clip.
    pub fn covers(&self, t: u64) -> bool {
        t >= self.start && t < self.end()
    }

    /// The source frame shown at timeline frame `t` (caller guarantees [`covers`]).
    ///
    /// [`covers`]: Clip::covers
    pub fn source_frame(&self, t: u64) -> u64 {
        self.src_in + (t - self.start)
    }

    /// Combined opacity at timeline frame `t`: `opacity` scaled by the head/tail
    /// fade ramps. Returns `0.0` outside the clip.
    pub fn video_alpha(&self, t: u64) -> f32 {
        self.opacity.clamp(0.0, 1.0) * self.fade_factor(t)
    }

    /// Combined audio gain at timeline frame `t`: `gain` scaled by the fade ramps,
    /// or `0.0` when muted/outside.
    pub fn audio_gain(&self, t: u64) -> f32 {
        if self.audio_muted {
            return 0.0;
        }
        self.gain.max(0.0) * self.fade_factor(t)
    }

    /// Linear fade ramp in `0.0..=1.0` from the head/tail fades at frame `t`.
    fn fade_factor(&self, t: u64) -> f32 {
        if !self.enabled || !self.covers(t) {
            return 0.0;
        }
        let into = t - self.start; // frames since the clip's head
        let dur = self.duration();
        let left = dur - 1 - into.min(dur - 1); // frames until the clip's tail
        let mut f = 1.0f32;
        if self.fade_in > 0 && into < self.fade_in {
            // +1 so the ramp reaches full at the first fully-opaque frame.
            f = f.min((into + 1) as f32 / (self.fade_in + 1) as f32);
        }
        if self.fade_out > 0 && left < self.fade_out {
            f = f.min((left + 1) as f32 / (self.fade_out + 1) as f32);
        }
        f.clamp(0.0, 1.0)
    }
}

/// Whether a track carries picture or sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackKind {
    /// Composited as picture (top track wins where opaque).
    Video,
    /// Summed into the mix.
    Audio,
}

/// One lane of non-overlapping clips.
#[derive(Clone, Debug)]
pub struct Track {
    /// Picture or sound.
    pub kind: TrackKind,
    /// Clips in start order; the model keeps them non-overlapping.
    pub clips: Vec<Clip>,
    /// Display name (e.g. `"V1"`, `"A1"`).
    pub name: String,
    /// Silenced in the mix (audio) — still shown.
    pub muted: bool,
    /// Hidden in the composite (video).
    pub hidden: bool,
    /// Locked against edits (advisory; enforced by the editor UI).
    pub locked: bool,
}

impl Track {
    /// An empty track of `kind` named `name`.
    pub fn new(kind: TrackKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            clips: Vec::new(),
            name: name.into(),
            muted: false,
            hidden: false,
            locked: false,
        }
    }

    /// Index of the clip covering timeline frame `t`, if any.
    pub fn clip_at(&self, t: u64) -> Option<usize> {
        self.clips.iter().position(|c| c.covers(t))
    }

    /// Re-sort clips by start frame (call after inserting).
    fn resort(&mut self) {
        self.clips.sort_by_key(|c| c.start);
    }

    /// End frame of the clip immediately to the left of `idx` in time (`0` if none).
    /// Clips never overlap, so every other clip is wholly left or wholly right.
    fn left_neighbor_end(&self, idx: usize) -> u64 {
        let start = self.clips[idx].start;
        self.clips
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != idx && o.end() <= start)
            .map(|(_, o)| o.end())
            .max()
            .unwrap_or(0)
    }

    /// Start frame of the clip immediately to the right of `idx` in time
    /// (`u64::MAX` if none).
    fn right_neighbor_start(&self, idx: usize) -> u64 {
        let end = self.clips[idx].end();
        self.clips
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != idx && o.start >= end)
            .map(|(_, o)| o.start)
            .min()
            .unwrap_or(u64::MAX)
    }
}

/// The whole edit: size, timing, audio format, and a bottom-to-top track stack.
#[derive(Clone, Debug)]
pub struct Timeline {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Output frame rate.
    pub fps: Rational,
    /// Project audio sample rate (Hz).
    pub sample_rate: u32,
    /// Project audio channel count.
    pub channels: u16,
    /// Tracks, composited **bottom-first** (`tracks[0]` is the bottom layer).
    pub tracks: Vec<Track>,
    /// Frame count of each registered media source, used to clamp tail trims to
    /// the available footage. Unknown ids are treated as unbounded.
    media_len: HashMap<MediaId, u64>,
}

impl Timeline {
    /// A timeline of `width`×`height` at `fps`, with the default 48 kHz stereo
    /// audio format and no tracks.
    pub fn new(width: u32, height: u32, fps: Rational) -> Self {
        Self {
            width,
            height,
            fps,
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
            tracks: Vec::new(),
            media_len: HashMap::new(),
        }
    }

    /// Record how many frames a media source has, so tail trims/extends can't run
    /// past the real footage. Call once per source as it's added to the project.
    pub fn register_media(&mut self, id: MediaId, frame_count: u64) {
        self.media_len.insert(id, frame_count);
    }

    /// Frames available in a clip's source, or `u64::MAX` for generated/unknown
    /// sources (which have no fixed length).
    fn source_len(&self, clip: &Clip) -> u64 {
        match &clip.source {
            Source::Media(id) => self.media_len.get(id).copied().unwrap_or(u64::MAX),
            _ => u64::MAX,
        }
    }

    /// Append a track and return its index.
    pub fn push_track(&mut self, kind: TrackKind, name: impl Into<String>) -> usize {
        self.tracks.push(Track::new(kind, name));
        self.tracks.len() - 1
    }

    /// Total length in frames (one past the last clip end across all tracks).
    pub fn duration_frames(&self) -> u64 {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(Clip::end)
            .max()
            .unwrap_or(0)
    }

    /// Whether any enabled clip on any non-muted track contributes audio (so an
    /// export knows to open an audio track).
    pub fn has_audio(&self) -> bool {
        self.tracks.iter().filter(|t| !t.muted).any(|t| {
            t.clips
                .iter()
                .any(|c| c.enabled && c.source.carries_audio())
        })
    }

    /// Total length in seconds.
    pub fn duration_secs(&self) -> f64 {
        let fps = self.fps.as_f64();
        if fps <= 0.0 {
            0.0
        } else {
            self.duration_frames() as f64 / fps
        }
    }

    /// Add a clip to track `track`, keeping the lane sorted by start frame.
    /// Returns `false` if the track index is out of range.
    pub fn add_clip(&mut self, track: usize, clip: Clip) -> bool {
        match self.tracks.get_mut(track) {
            Some(t) => {
                t.clips.push(clip);
                t.resort();
                true
            }
            None => false,
        }
    }

    /// Split whichever clip on `track` covers frame `t` into two clips at `t`.
    /// A split exactly on a clip boundary is a no-op. Returns `true` if a clip
    /// was split.
    pub fn split(&mut self, track: usize, t: u64) -> bool {
        let Some(tr) = self.tracks.get_mut(track) else {
            return false;
        };
        let Some(i) = tr.clip_at(t) else {
            return false;
        };
        // Splitting at the very start produces no left piece — nothing to do.
        if t <= tr.clips[i].start {
            return false;
        }
        let left = &mut tr.clips[i];
        let split_src = left.source_frame(t); // first source frame of the right half
        let mut right = left.clone();
        // Left keeps its head; its tail (and any fade-out) moves to the right half.
        let left_fade_out = std::mem::take(&mut left.fade_out);
        left.src_out = split_src;
        // Right starts at the cut, keeping the remaining source + the tail fade.
        right.src_in = split_src;
        right.start = t;
        right.fade_in = 0;
        right.fade_out = left_fade_out;
        tr.clips.insert(i + 1, right);
        true
    }

    /// Remove clip `clip` from `track`, leaving a gap. Returns the removed clip.
    pub fn remove_clip(&mut self, track: usize, clip: usize) -> Option<Clip> {
        let tr = self.tracks.get_mut(track)?;
        if clip < tr.clips.len() {
            Some(tr.clips.remove(clip))
        } else {
            None
        }
    }

    /// Ripple-delete clip `clip` on `track`: remove it and pull every clip that
    /// started at or after it on the **same** track left by the deleted duration
    /// (closing the gap). Order-independent, so it's safe after moves/trims.
    /// Returns the removed clip.
    pub fn ripple_delete(&mut self, track: usize, clip: usize) -> Option<Clip> {
        let tr = self.tracks.get_mut(track)?;
        if clip >= tr.clips.len() {
            return None;
        }
        let removed = tr.clips.remove(clip);
        let shift = removed.duration();
        for c in tr.clips.iter_mut() {
            if c.start >= removed.start {
                c.start = c.start.saturating_sub(shift);
            }
        }
        Some(removed)
    }

    /// Move clip `clip` on `track` to (or toward) timeline frame `target_start`,
    /// keeping its content. The move is clamped to the gap between its immediate
    /// neighbours so clips never overlap. Indices are preserved (no re-sort), so a
    /// held selection stays valid. Returns `false` for a bad index.
    pub fn move_clip(&mut self, track: usize, clip: usize, target_start: u64) -> bool {
        let Some(tr) = self.tracks.get_mut(track) else {
            return false;
        };
        if clip >= tr.clips.len() {
            return false;
        }
        let dur = tr.clips[clip].duration();
        let lo = tr.left_neighbor_end(clip);
        let hi = tr.right_neighbor_start(clip).saturating_sub(dur).max(lo);
        tr.clips[clip].start = target_start.clamp(lo, hi);
        true
    }

    /// Trim (or extend) clip `clip`'s **head** by dragging its left edge to
    /// timeline frame `target_start`, anchoring the tail. Clamped to the left
    /// neighbour, the source's first frame, and a one-frame minimum.
    pub fn trim_head(&mut self, track: usize, clip: usize, target_start: u64) -> bool {
        let Some(tr) = self.tracks.get_mut(track) else {
            return false;
        };
        if clip >= tr.clips.len() {
            return false;
        }
        let ln = tr.left_neighbor_end(clip);
        let c = &mut tr.clips[clip];
        let end = c.end();
        // Leftmost the head may reach: not past the left neighbour, and not before
        // the source's first frame (start - src_in). Never past the tail (min 1f).
        let lo = ln.max(c.start.saturating_sub(c.src_in));
        let hi = end.saturating_sub(1);
        let ns = target_start.clamp(lo.min(hi), hi);
        c.src_in = c.src_out - (end - ns);
        c.start = ns;
        true
    }

    /// Trim (or extend) clip `clip`'s **tail** by dragging its right edge to
    /// timeline frame `target_end`, anchoring the head. Clamped to the right
    /// neighbour, the source's length, and a one-frame minimum.
    pub fn trim_tail(&mut self, track: usize, clip: usize, target_end: u64) -> bool {
        let max_out = match self.tracks.get(track).and_then(|t| t.clips.get(clip)) {
            Some(c) => self.source_len(c),
            None => return false,
        };
        let Some(tr) = self.tracks.get_mut(track) else {
            return false;
        };
        let rn = tr.right_neighbor_start(clip);
        let c = &mut tr.clips[clip];
        let lo = c.start + 1;
        // Rightmost the tail may reach: not into the right neighbour, and not past
        // the available footage (start + (source_len - src_in)).
        let src_room = c.start.saturating_add(max_out.saturating_sub(c.src_in));
        let hi = rn.min(src_room).max(lo);
        let ne = target_end.clamp(lo, hi);
        c.src_out = c.src_in + (ne - c.start);
        true
    }

    /// Convert a timeline frame index to the project audio sample index (per
    /// channel) at that frame's start, using exact integer math.
    pub fn frame_to_sample(&self, frame: u64) -> u64 {
        if self.fps.num == 0 {
            return 0;
        }
        let n = frame as u128 * self.sample_rate as u128 * self.fps.den as u128;
        (n / self.fps.num as u128) as u64
    }

    /// Convert a per-channel audio sample index back to the timeline frame it falls
    /// in (the inverse of [`frame_to_sample`], rounded down).
    ///
    /// [`frame_to_sample`]: Timeline::frame_to_sample
    pub fn sample_to_frame(&self, sample: u64) -> u64 {
        let denom = self.sample_rate as u128 * self.fps.den as u128;
        if denom == 0 {
            return 0;
        }
        let n = sample as u128 * self.fps.num as u128;
        (n / denom) as u64
    }
}

/// A frame index rendered as `HH:MM:SS:FF` plus the absolute frame number (B4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timecode {
    /// Whole hours.
    pub hours: u64,
    /// Minutes `0..60`.
    pub minutes: u64,
    /// Seconds `0..60`.
    pub seconds: u64,
    /// Frames within the second.
    pub frames: u64,
    /// Absolute frame index from the start.
    pub frame: u64,
}

impl Timecode {
    /// Decompose `frame` at `fps` into `HH:MM:SS:FF`.
    pub fn from_frame(frame: u64, fps: Rational) -> Self {
        let rate = fps.as_f64().round().max(1.0) as u64;
        let total_secs = frame / rate;
        Self {
            hours: total_secs / 3600,
            minutes: (total_secs / 60) % 60,
            seconds: total_secs % 60,
            frames: frame % rate,
            frame,
        }
    }

    /// `HH:MM:SS:FF`.
    pub fn smpte(&self) -> String {
        format!(
            "{:02}:{:02}:{:02}:{:02}",
            self.hours, self.minutes, self.seconds, self.frames
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline_30fps() -> Timeline {
        Timeline::new(64, 48, Rational::new(30, 1))
    }

    #[test]
    fn clip_geometry() {
        let c = Clip::media(MediaId(1), 10, 40, 100);
        assert_eq!(c.duration(), 30);
        assert_eq!(c.end(), 130);
        assert!(c.covers(100));
        assert!(c.covers(129));
        assert!(!c.covers(130));
        assert!(!c.covers(99));
        assert_eq!(c.source_frame(100), 10);
        assert_eq!(c.source_frame(129), 39);
    }

    #[test]
    fn duration_is_max_clip_end() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        let a = tl.push_track(TrackKind::Audio, "A1");
        tl.add_clip(v, Clip::media(MediaId(1), 0, 30, 0));
        tl.add_clip(a, Clip::media(MediaId(1), 0, 90, 10));
        assert_eq!(tl.duration_frames(), 100); // audio clip 10..100 wins
    }

    #[test]
    fn add_clip_keeps_lane_sorted() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.add_clip(v, Clip::media(MediaId(1), 0, 10, 100));
        tl.add_clip(v, Clip::media(MediaId(1), 0, 10, 0));
        tl.add_clip(v, Clip::media(MediaId(1), 0, 10, 50));
        let starts: Vec<u64> = tl.tracks[v].clips.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![0, 50, 100]);
    }

    #[test]
    fn split_divides_one_clip_into_two() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.add_clip(v, Clip::media(MediaId(7), 100, 200, 0)); // 100 frames, source 100..200
        assert!(tl.split(v, 40));
        let clips = &tl.tracks[v].clips;
        assert_eq!(clips.len(), 2);
        // Left: timeline 0..40, source 100..140.
        assert_eq!((clips[0].start, clips[0].end()), (0, 40));
        assert_eq!((clips[0].src_in, clips[0].src_out), (100, 140));
        // Right: timeline 40..100, source 140..200.
        assert_eq!((clips[1].start, clips[1].end()), (40, 100));
        assert_eq!((clips[1].src_in, clips[1].src_out), (140, 200));
    }

    #[test]
    fn split_on_boundary_is_noop() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.add_clip(v, Clip::media(MediaId(1), 0, 50, 0));
        assert!(!tl.split(v, 0)); // at the head
        assert!(!tl.split(v, 50)); // at/after the tail (no clip covers 50)
        assert_eq!(tl.tracks[v].clips.len(), 1);
    }

    #[test]
    fn split_moves_tail_fade_to_right_half() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        let mut c = Clip::media(MediaId(1), 0, 100, 0);
        c.fade_in = 5;
        c.fade_out = 8;
        tl.add_clip(v, c);
        tl.split(v, 60);
        let clips = &tl.tracks[v].clips;
        assert_eq!((clips[0].fade_in, clips[0].fade_out), (5, 0));
        assert_eq!((clips[1].fade_in, clips[1].fade_out), (0, 8));
    }

    #[test]
    fn ripple_delete_closes_the_gap() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.add_clip(v, Clip::media(MediaId(1), 0, 30, 0)); // 0..30
        tl.add_clip(v, Clip::media(MediaId(2), 0, 30, 30)); // 30..60
        tl.add_clip(v, Clip::media(MediaId(3), 0, 30, 60)); // 60..90
        let removed = tl.ripple_delete(v, 1).unwrap();
        assert_eq!(removed.duration(), 30);
        let starts: Vec<u64> = tl.tracks[v].clips.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![0, 30]); // third clip pulled left to close the gap
        assert_eq!(tl.duration_frames(), 60);
    }

    #[test]
    fn remove_clip_leaves_a_gap() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.add_clip(v, Clip::media(MediaId(1), 0, 30, 0));
        tl.add_clip(v, Clip::media(MediaId(2), 0, 30, 30));
        tl.remove_clip(v, 0);
        assert_eq!(tl.tracks[v].clips.len(), 1);
        assert_eq!(tl.tracks[v].clips[0].start, 30); // gap at 0..30 remains
    }

    fn two_clip_lane() -> (Timeline, usize) {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.add_clip(v, Clip::media(MediaId(1), 0, 30, 0)); // A: 0..30
        tl.add_clip(v, Clip::media(MediaId(2), 0, 30, 50)); // B: 50..80
        (tl, v)
    }

    #[test]
    fn move_clip_left_butts_against_neighbour_keeping_content() {
        let (mut tl, v) = two_clip_lane();
        // Pull B left toward 10 → butts against A's end (30), content unchanged.
        assert!(tl.move_clip(v, 1, 10));
        let b = &tl.tracks[v].clips[1];
        assert_eq!(b.start, 30);
        assert_eq!((b.src_in, b.src_out), (0, 30));
    }

    #[test]
    fn move_clip_right_is_blocked_by_neighbour() {
        let (mut tl, v) = two_clip_lane();
        // Push A right toward 100 → blocked by B at 50 (latest start = 50 - 30).
        assert!(tl.move_clip(v, 0, 100));
        assert_eq!(tl.tracks[v].clips[0].start, 20);
    }

    #[test]
    fn trim_head_anchors_the_tail() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.add_clip(v, Clip::media(MediaId(1), 10, 40, 100)); // [100,130], src 10..40
                                                              // Drag the head right to 110 → src_in advances, tail (130 / src_out 40) fixed.
        assert!(tl.trim_head(v, 0, 110));
        let c = &tl.tracks[v].clips[0];
        assert_eq!((c.start, c.end()), (110, 130));
        assert_eq!((c.src_in, c.src_out), (20, 40));
    }

    #[test]
    fn trim_head_reveals_source_but_stops_at_frame_zero() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.add_clip(v, Clip::media(MediaId(1), 10, 40, 100)); // 10 frames of head room
                                                              // Drag the head far left → clamps at source frame 0 (start can drop by 10).
        assert!(tl.trim_head(v, 0, 0));
        let c = &tl.tracks[v].clips[0];
        assert_eq!((c.start, c.src_in, c.src_out), (90, 0, 40));
    }

    #[test]
    fn trim_tail_clamps_to_registered_source_length() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.register_media(MediaId(1), 50); // source has 50 frames
        tl.add_clip(v, Clip::media(MediaId(1), 0, 30, 0)); // [0,30], src 0..30
                                                           // Try to extend the tail to 80 → clamped to the 50-frame source.
        assert!(tl.trim_tail(v, 0, 80));
        let c = &tl.tracks[v].clips[0];
        assert_eq!((c.start, c.end(), c.src_out), (0, 50, 50));
    }

    #[test]
    fn trim_tail_clamps_to_the_right_neighbour() {
        let mut tl = timeline_30fps();
        let v = tl.push_track(TrackKind::Video, "V1");
        tl.register_media(MediaId(1), 1000);
        tl.add_clip(v, Clip::media(MediaId(1), 0, 30, 0)); // A: 0..30
        tl.add_clip(v, Clip::media(MediaId(2), 0, 30, 40)); // B: 40..70
                                                            // Extend A's tail toward 60 → blocked by B's start (40).
        assert!(tl.trim_tail(v, 0, 60));
        assert_eq!(tl.tracks[v].clips[0].end(), 40);
    }

    #[test]
    fn fade_in_ramps_alpha_from_low_to_full() {
        let mut c = Clip::media(MediaId(1), 0, 100, 0);
        c.fade_in = 4;
        // Monotonic non-decreasing across the fade, reaching 1.0 by the end.
        let a0 = c.video_alpha(0);
        let a3 = c.video_alpha(3);
        let a_full = c.video_alpha(10);
        assert!(a0 > 0.0 && a0 < a3, "{a0} !< {a3}");
        assert!((a_full - 1.0).abs() < 1e-6);
    }

    #[test]
    fn fade_out_ramps_alpha_down_at_the_tail() {
        let mut c = Clip::media(MediaId(1), 0, 100, 0);
        c.fade_out = 4;
        assert!((c.video_alpha(50) - 1.0).abs() < 1e-6);
        let last = c.video_alpha(99);
        let near = c.video_alpha(96);
        assert!(last > 0.0 && last < near, "{last} !< {near}");
    }

    #[test]
    fn disabled_clip_is_fully_transparent_and_silent() {
        let mut c = Clip::media(MediaId(1), 0, 100, 0);
        c.enabled = false;
        assert_eq!(c.video_alpha(10), 0.0);
        assert_eq!(c.audio_gain(10), 0.0);
    }

    #[test]
    fn muted_clip_has_zero_audio_gain_but_shows_video() {
        let mut c = Clip::media(MediaId(1), 0, 100, 0);
        c.audio_muted = true;
        assert_eq!(c.audio_gain(10), 0.0);
        assert!(c.video_alpha(10) > 0.0);
    }

    #[test]
    fn frame_to_sample_is_exact_for_integer_fps() {
        let tl = timeline_30fps(); // 48000 / 30 = 1600 samples per frame
        assert_eq!(tl.frame_to_sample(0), 0);
        assert_eq!(tl.frame_to_sample(1), 1600);
        assert_eq!(tl.frame_to_sample(30), 48_000);
    }

    #[test]
    fn timecode_formats_smpte() {
        let fps = Rational::new(30, 1);
        assert_eq!(Timecode::from_frame(0, fps).smpte(), "00:00:00:00");
        assert_eq!(Timecode::from_frame(45, fps).smpte(), "00:00:01:15");
        assert_eq!(
            Timecode::from_frame(3600 * 30 + 1, fps).smpte(),
            "01:00:00:01"
        );
    }
}
