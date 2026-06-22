//! The compositor: turn the [`Timeline`] model + decoded media (via a
//! [`MediaProvider`]) into a final RGBA [`Frame`] at a given timeline frame, and a
//! mixed block of audio for a sample range. Deterministic and headless, so the
//! same call renders identically in the live preview and at export (WYSIWYG).
//!
//! Video tracks composite **bottom-first**: `tracks[0]` is the base, later tracks
//! draw over it with per-clip opacity and head/tail fades (so a clip fading in on
//! an upper track is a cross-dissolve over the track below). Source frames are
//! aspect-fit (letterboxed) when their size differs from the timeline's. Audio
//! sums every audio-bearing clip on every non-muted track — including the linked
//! audio of video clips — with per-clip gain + fades, then clamps to `i16`.

use freally_video::Frame;

use crate::{MediaProvider, Source, Timeline, TrackKind};

/// Render the composited timeline frame at timeline frame `t`.
///
/// Areas no clip covers are opaque black. Unknown/again-out-of-range media frames
/// are skipped (the layer below shows through).
pub fn compose_frame(timeline: &Timeline, provider: &mut dyn MediaProvider, t: u64) -> Frame {
    let (w, h) = (timeline.width.max(1), timeline.height.max(1));
    // Opaque black base.
    let mut canvas = vec![0u8; w as usize * h as usize * 4];
    for px in canvas.chunks_exact_mut(4) {
        px[3] = 255;
    }

    for track in &timeline.tracks {
        if track.kind != TrackKind::Video || track.hidden {
            continue;
        }
        let Some(i) = track.clip_at(t) else { continue };
        let clip = &track.clips[i];
        let alpha = clip.video_alpha(t);
        if alpha <= 0.0 {
            continue;
        }
        // Global opacity as 0..=255 so a fully-opaque source stays exact.
        let alpha255 = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;

        match &clip.source {
            Source::Color(rgba) => blend_solid(&mut canvas, *rgba, alpha255),
            Source::Still(frame) => blit(&mut canvas, w, h, frame, alpha255),
            Source::Media(id) => {
                if let Some(frame) = provider.frame(*id, clip.source_frame(t)) {
                    blit(&mut canvas, w, h, &frame, alpha255);
                }
            }
        }
    }

    Frame::new(w, h, canvas).expect("canvas is width*height*4 by construction")
}

/// Mix `len` per-channel audio sample-frames starting at project sample
/// `start_sample`. The result is interleaved at `timeline.channels`, always
/// exactly `len * channels` samples long, clamped to `i16`.
pub fn compose_audio(
    timeline: &Timeline,
    provider: &mut dyn MediaProvider,
    start_sample: u64,
    len: usize,
) -> Vec<i16> {
    let ch = timeline.channels.max(1) as usize;
    let mut acc = vec![0i32; len * ch];

    for track in &timeline.tracks {
        if track.muted {
            continue;
        }
        for clip in &track.clips {
            if !clip.enabled || !clip.source.carries_audio() {
                continue;
            }
            let Source::Media(id) = &clip.source else {
                continue;
            };
            let clip_start = timeline.frame_to_sample(clip.start);
            let clip_end = timeline.frame_to_sample(clip.end());
            let region_start = start_sample.max(clip_start);
            let region_end = (start_sample + len as u64).min(clip_end);
            if region_start >= region_end {
                continue;
            }
            let n = (region_end - region_start) as usize;
            let src_start = timeline.frame_to_sample(clip.src_in) + (region_start - clip_start);
            let block = provider.samples(*id, src_start, n, timeline.channels);

            for k in 0..n {
                let global = region_start + k as u64;
                let out = (global - start_sample) as usize;
                // Fades are frame-granular: map this sample back to its timeline frame.
                let g = clip.audio_gain(timeline.sample_to_frame(global));
                let g256 = (g.clamp(0.0, 4.0) * 256.0) as i32;
                for c in 0..ch {
                    let s = block[k * ch + c] as i32;
                    acc[out * ch + c] += (s * g256) >> 8;
                }
            }
        }
    }

    acc.iter().map(|&v| v.clamp(-32768, 32767) as i16).collect()
}

/// Source-over alpha blend of one pixel: `dst = src*a + dst*(255-a)`, with `a` the
/// pixel's effective coverage in `0..=255`. Exact rounded `/255` math so a fully
/// opaque source reproduces its colour exactly (no 255-vs-256 drift).
#[inline]
fn over(dst: &mut [u8], src: [u8; 3], a: u32) {
    let inv = 255 - a;
    for c in 0..3 {
        dst[c] = ((src[c] as u32 * a + dst[c] as u32 * inv + 127) / 255) as u8;
    }
    dst[3] = ((255 * a + dst[3] as u32 * inv + 127) / 255).min(255) as u8;
}

/// Combine a source pixel's own alpha (`src_a`, 0..=255) with the clip's global
/// opacity (`alpha255`, 0..=255) into a single coverage value in `0..=255`.
#[inline]
fn coverage(src_a: u8, alpha255: u32) -> u32 {
    (src_a as u32 * alpha255 + 127) / 255
}

/// Alpha-blend a solid RGBA colour over the whole canvas.
fn blend_solid(canvas: &mut [u8], rgba: [u8; 4], alpha255: u32) {
    let a = coverage(rgba[3], alpha255);
    if a == 0 {
        return;
    }
    let rgb = [rgba[0], rgba[1], rgba[2]];
    for px in canvas.chunks_exact_mut(4) {
        over(px, rgb, a);
    }
}

/// Aspect-fit (letterbox) `src` onto the `cw`×`ch` canvas with global opacity
/// `alpha255` (0..=255), alpha-blending per pixel. Nearest-neighbour sampling;
/// a same-size source maps 1:1.
fn blit(canvas: &mut [u8], cw: u32, ch: u32, src: &Frame, alpha255: u32) {
    let (sw, sh) = (src.width.max(1), src.height.max(1));
    // Largest integer-rounded box that fits, preserving aspect ratio.
    let scale = (cw as f32 / sw as f32).min(ch as f32 / sh as f32);
    let dw = ((sw as f32 * scale).round() as u32).clamp(1, cw);
    let dh = ((sh as f32 * scale).round() as u32).clamp(1, ch);
    let ox = (cw - dw) / 2;
    let oy = (ch - dh) / 2;

    for dy in 0..dh {
        let sy = (dy as u64 * sh as u64 / dh as u64).min(sh as u64 - 1) as u32;
        let src_row = (sy * sw) as usize * 4;
        let dst_row = ((oy + dy) * cw + ox) as usize * 4;
        for dx in 0..dw {
            let sx = (dx as u64 * sw as u64 / dw as u64).min(sw as u64 - 1) as u32;
            let s = &src.pixels[src_row + sx as usize * 4..][..4];
            let a = coverage(s[3], alpha255);
            if a == 0 {
                continue;
            }
            over(
                &mut canvas[dst_row + dx as usize * 4..][..4],
                [s[0], s[1], s[2]],
                a,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Clip, MediaId, MemoryProvider, Timeline};
    use freally_video::{AudioTrack, Frame, Movie, Rational};
    use std::sync::Arc;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Frame {
        Frame::from_rgba(w, h, &rgba.repeat((w * h) as usize)).unwrap()
    }

    /// Top-left pixel of a composited frame.
    fn px0(f: &Frame) -> [u8; 4] {
        [f.pixels[0], f.pixels[1], f.pixels[2], f.pixels[3]]
    }

    fn tl() -> Timeline {
        Timeline::new(4, 4, Rational::new(30, 1))
    }

    #[test]
    fn empty_timeline_renders_opaque_black() {
        let mut p = MemoryProvider::new();
        let f = compose_frame(&tl(), &mut p, 0);
        assert_eq!(px0(&f), [0, 0, 0, 255]);
    }

    #[test]
    fn gap_between_clips_is_black() {
        let mut t = tl();
        let v = t.push_track(TrackKind::Video, "V1");
        t.add_clip(v, Clip::color([255, 0, 0, 255], 10, 0)); // 0..10
        t.add_clip(v, Clip::color([0, 255, 0, 255], 10, 20)); // 20..30
        let mut p = MemoryProvider::new();
        assert_eq!(px0(&compose_frame(&t, &mut p, 5)), [255, 0, 0, 255]);
        assert_eq!(px0(&compose_frame(&t, &mut p, 15)), [0, 0, 0, 255]); // the gap
        assert_eq!(px0(&compose_frame(&t, &mut p, 25)), [0, 255, 0, 255]);
    }

    #[test]
    fn upper_track_draws_over_lower() {
        let mut t = tl();
        let lo = t.push_track(TrackKind::Video, "V1");
        let hi = t.push_track(TrackKind::Video, "V2");
        t.add_clip(lo, Clip::color([255, 0, 0, 255], 10, 0));
        t.add_clip(hi, Clip::color([0, 0, 255, 255], 10, 0));
        let mut p = MemoryProvider::new();
        assert_eq!(px0(&compose_frame(&t, &mut p, 5)), [0, 0, 255, 255]); // top wins
    }

    #[test]
    fn hidden_track_is_skipped() {
        let mut t = tl();
        let lo = t.push_track(TrackKind::Video, "V1");
        let hi = t.push_track(TrackKind::Video, "V2");
        t.add_clip(lo, Clip::color([255, 0, 0, 255], 10, 0));
        t.add_clip(hi, Clip::color([0, 0, 255, 255], 10, 0));
        t.tracks[hi].hidden = true;
        let mut p = MemoryProvider::new();
        assert_eq!(px0(&compose_frame(&t, &mut p, 5)), [255, 0, 0, 255]); // top hidden
    }

    #[test]
    fn half_opacity_blends_evenly() {
        let mut t = tl();
        let lo = t.push_track(TrackKind::Video, "V1");
        let hi = t.push_track(TrackKind::Video, "V2");
        t.add_clip(lo, Clip::color([0, 0, 0, 255], 10, 0));
        let mut top = Clip::color([255, 255, 255, 255], 10, 0);
        top.opacity = 0.5;
        t.add_clip(hi, top);
        let mut p = MemoryProvider::new();
        let got = px0(&compose_frame(&t, &mut p, 5));
        // ~50% white over black → mid grey (allow rounding slack).
        assert!((got[0] as i32 - 128).abs() <= 2, "{got:?}");
    }

    #[test]
    fn media_frames_are_pulled_at_the_right_source_index() {
        let mut t = tl();
        let v = t.push_track(TrackKind::Video, "V1");
        // Source frames: 0=red, 1=green, 2=blue.
        let frames = vec![
            solid(4, 4, [200, 0, 0, 255]),
            solid(4, 4, [0, 200, 0, 255]),
            solid(4, 4, [0, 0, 200, 255]),
        ];
        let mut p = MemoryProvider::new();
        p.insert(
            MediaId(1),
            Movie::new(4, 4, Rational::new(30, 1), frames, None),
        );
        // Show source 1..3 starting at timeline frame 10.
        t.add_clip(v, Clip::media(MediaId(1), 1, 3, 10));
        assert_eq!(px0(&compose_frame(&t, &mut p, 10)), [0, 200, 0, 255]); // src 1
        assert_eq!(px0(&compose_frame(&t, &mut p, 11)), [0, 0, 200, 255]); // src 2
    }

    #[test]
    fn wide_source_is_letterboxed_top_and_bottom() {
        let mut t = Timeline::new(4, 4, Rational::new(30, 1));
        let v = t.push_track(TrackKind::Video, "V1");
        // A 4x2 (wide 2:1) white still on a 4x4 canvas → fits the width, leaving a
        // black bar on the top row (y=0) and bottom row (y=3); rows 1–2 are white.
        let still = Arc::new(solid(4, 2, [255, 255, 255, 255]));
        t.add_clip(v, Clip::still(still, 10, 0));
        let mut p = MemoryProvider::new();
        let f = compose_frame(&t, &mut p, 0);
        let row = |y: u32, x: u32| {
            let i = ((y * 4 + x) * 4) as usize;
            [
                f.pixels[i],
                f.pixels[i + 1],
                f.pixels[i + 2],
                f.pixels[i + 3],
            ]
        };
        assert_eq!(row(0, 0), [0, 0, 0, 255]); // top letterbox bar
        assert_eq!(row(3, 0), [0, 0, 0, 255]); // bottom letterbox bar
        assert_eq!(row(1, 1), [255, 255, 255, 255]); // image band
        assert_eq!(row(2, 2), [255, 255, 255, 255]);
    }

    #[test]
    fn equal_aspect_source_upscales_to_fill() {
        let mut t = Timeline::new(4, 4, Rational::new(30, 1));
        let v = t.push_track(TrackKind::Video, "V1");
        // A 2x2 white still on a 4x4 canvas has the same 1:1 aspect → fills it, no
        // letterbox (every corner is white).
        let still = Arc::new(solid(2, 2, [255, 255, 255, 255]));
        t.add_clip(v, Clip::still(still, 10, 0));
        let mut p = MemoryProvider::new();
        let f = compose_frame(&t, &mut p, 0);
        assert_eq!(px0(&f), [255, 255, 255, 255]);
    }

    #[test]
    fn audio_sums_clips_across_tracks() {
        let mut t = tl();
        let a1 = t.push_track(TrackKind::Audio, "A1");
        let a2 = t.push_track(TrackKind::Audio, "A2");
        // Two mono sources, each conformed to stereo by the provider.
        let mut p = MemoryProvider::new();
        p.insert(MediaId(1), const_audio_movie(100, 2)); // value 100, stereo
        p.insert(MediaId(2), const_audio_movie(50, 2));
        t.add_clip(a1, Clip::media(MediaId(1), 0, 30, 0));
        t.add_clip(a2, Clip::media(MediaId(2), 0, 30, 0));
        let mixed = compose_audio(&t, &mut p, 0, 4);
        // 100 + 50 on both channels.
        assert_eq!(mixed, vec![150, 150, 150, 150, 150, 150, 150, 150]);
    }

    #[test]
    fn muted_track_contributes_no_audio() {
        let mut t = tl();
        let a1 = t.push_track(TrackKind::Audio, "A1");
        let mut p = MemoryProvider::new();
        p.insert(MediaId(1), const_audio_movie(100, 2));
        t.add_clip(a1, Clip::media(MediaId(1), 0, 30, 0));
        t.tracks[a1].muted = true;
        assert_eq!(compose_audio(&t, &mut p, 0, 4), vec![0; 8]);
    }

    #[test]
    fn clip_gain_scales_audio() {
        let mut t = tl();
        let a1 = t.push_track(TrackKind::Audio, "A1");
        let mut p = MemoryProvider::new();
        p.insert(MediaId(1), const_audio_movie(100, 2));
        let mut c = Clip::media(MediaId(1), 0, 30, 0);
        c.gain = 0.5;
        t.add_clip(a1, c);
        assert_eq!(compose_audio(&t, &mut p, 0, 2), vec![50, 50, 50, 50]);
    }

    #[test]
    fn audio_outside_clip_span_is_silent() {
        let mut t = tl();
        let a1 = t.push_track(TrackKind::Audio, "A1");
        let mut p = MemoryProvider::new();
        p.insert(MediaId(1), const_audio_movie(100, 2));
        // Clip occupies timeline frames 1..2 → samples 1600..3200 at 30fps/48k.
        t.add_clip(a1, Clip::media(MediaId(1), 0, 1, 1));
        // Sample 0 is before the clip → silent; sample 1600 is inside → 100.
        assert_eq!(compose_audio(&t, &mut p, 0, 1), vec![0, 0]);
        assert_eq!(compose_audio(&t, &mut p, 1600, 1), vec![100, 100]);
    }

    /// A movie with no frames and a constant-valued audio track long enough for
    /// the tests (1 second), used to check the mixer.
    fn const_audio_movie(value: i16, channels: u16) -> Movie {
        let per_ch = 48_000usize;
        let samples = vec![value; per_ch * channels as usize];
        Movie::new(
            0,
            0,
            Rational::new(30, 1),
            Vec::new(),
            Some(AudioTrack::new(48_000, channels, samples)),
        )
    }
}
