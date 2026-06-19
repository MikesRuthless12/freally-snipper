//! Export a `.fvid` recording to a shareable video (P5.1, optional).
//!
//! - **GIF** — encoded in-process with the `image` crate (patent-free; downscaled
//!   for a sane size). No external dependency.
//! - **WebM (VP9/Opus)** and **MP4 (H.264/AAC)** — encoded by **ffmpeg**, run as a
//!   **separate subprocess** (the proprietary app is never linked to ffmpeg, so it
//!   stays at arm's length from ffmpeg's GPL/LGPL — only the standalone binary
//!   carries that license). **WebM is royalty-free**; **MP4**'s H.264/AAC are
//!   patent-pooled (the owner accepts that responsibility). ffmpeg is fetched on
//!   demand the first time an MP4/WebM export runs.
//!
//! Frames stream from the `.fvid` one at a time (so a 4K export never holds the
//! whole movie in RAM); audio is read with [`freally_video::read_audio_file`] and
//! muxed via a temporary WAV.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use ffmpeg_sidecar::command::FfmpegCommand;
use freally_video::{read_audio_file, AudioTrack, StreamDecoder};

/// Largest GIF width; wider recordings are downscaled (a 4K GIF would be absurd).
const MAX_GIF_WIDTH: u32 = 640;

/// A target export format.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Animated GIF (in-process, patent-free).
    Gif,
    /// WebM — VP9 video + Opus audio (royalty-free).
    WebM,
    /// MP4 — H.264 video + AAC audio (most compatible; patent-pooled).
    Mp4,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 3] = [Self::WebM, Self::Mp4, Self::Gif];

    /// Lower-case file extension.
    pub fn ext(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::WebM => "webm",
            Self::Mp4 => "mp4",
        }
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Gif => "Animated GIF",
            Self::WebM => "WebM (VP9/Opus)",
            Self::Mp4 => "MP4 (H.264/AAC)",
        }
    }
}

/// A message from the export worker.
enum ExportMsg {
    Progress(f32),
    Done(Result<PathBuf, String>),
}

/// A running export, polled by the player each frame.
pub struct ExportJob {
    rx: Receiver<ExportMsg>,
    pub format: ExportFormat,
    pub progress: f32,
    /// `Some` once finished: the written path, or an error message.
    pub done: Option<Result<PathBuf, String>>,
}

impl ExportJob {
    /// Spawn an export of `src` to `dst` in `format`.
    pub fn start(ctx: &egui::Context, src: PathBuf, dst: PathBuf, format: ExportFormat) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("freally-export".to_owned())
            .spawn(move || {
                let result = run_export(&src, &dst, format, &tx, &ctx);
                let _ = tx.send(ExportMsg::Done(result));
                ctx.request_repaint();
            });
        if spawned.is_err() {
            // Report immediately via the channel.
            let (tx2, rx2) = mpsc::channel();
            let _ = tx2.send(ExportMsg::Done(Err(
                "Couldn't start the export thread".to_owned()
            )));
            return Self {
                rx: rx2,
                format,
                progress: 0.0,
                done: None,
            };
        }
        Self {
            rx,
            format,
            progress: 0.0,
            done: None,
        }
    }

    /// Drain progress/result updates. Call each frame while the player is open.
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ExportMsg::Progress(p) => self.progress = p,
                ExportMsg::Done(result) => self.done = Some(result),
            }
        }
    }
}

/// Run the export to completion, returning the written path.
fn run_export(
    src: &Path,
    dst: &Path,
    format: ExportFormat,
    tx: &mpsc::Sender<ExportMsg>,
    ctx: &egui::Context,
) -> Result<PathBuf, String> {
    match format {
        ExportFormat::Gif => export_gif(src, dst, tx, ctx),
        ExportFormat::WebM | ExportFormat::Mp4 => export_ffmpeg(src, dst, format, tx, ctx),
    }
}

/// Encode the recording to an animated GIF with the `image` crate (no ffmpeg).
fn export_gif(
    src: &Path,
    dst: &Path,
    tx: &mpsc::Sender<ExportMsg>,
    ctx: &egui::Context,
) -> Result<PathBuf, String> {
    use image::codecs::gif::{GifEncoder, Repeat};

    let mut decoder = StreamDecoder::open(src).map_err(|e| e.to_string())?;
    let (w, h) = (decoder.width(), decoder.height());
    let fps = decoder.fps().as_f64().max(1.0);
    let count = decoder.frame_count().max(1);
    let (gw, gh) = gif_size(w, h);

    let file = std::fs::File::create(dst).map_err(|e| format!("Couldn't create {dst:?}: {e}"))?;
    let mut encoder = GifEncoder::new_with_speed(BufWriter::new(file), 10);
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(|e| format!("GIF error: {e}"))?;
    let delay = image::Delay::from_numer_denom_ms((1000.0 / fps).round().max(1.0) as u32, 1);

    let mut done = 0u32;
    while let Some(frame) = decoder.next_frame().map_err(|e| e.to_string())? {
        let image = image::RgbaImage::from_raw(w, h, frame.pixels)
            .ok_or_else(|| "frame had the wrong size".to_owned())?;
        let image = if (gw, gh) == (w, h) {
            image
        } else {
            image::imageops::resize(&image, gw, gh, image::imageops::FilterType::Triangle)
        };
        encoder
            .encode_frame(image::Frame::from_parts(image, 0, 0, delay))
            .map_err(|e| format!("GIF encode failed: {e}"))?;
        done += 1;
        let _ = tx.send(ExportMsg::Progress(done as f32 / count as f32));
        ctx.request_repaint();
    }
    Ok(dst.to_path_buf())
}

/// Encode the recording with ffmpeg (subprocess): frames are piped in as raw
/// RGBA, audio (if any) is muxed from a temporary WAV.
fn export_ffmpeg(
    src: &Path,
    dst: &Path,
    format: ExportFormat,
    tx: &mpsc::Sender<ExportMsg>,
    ctx: &egui::Context,
) -> Result<PathBuf, String> {
    // Fetch ffmpeg on first use (downloaded to a per-user cache, not bundled).
    ffmpeg_sidecar::download::auto_download().map_err(|e| {
        format!(
            "Couldn't obtain ffmpeg (needed for {} export): {e}",
            format.label()
        )
    })?;

    // Read the header for size/fps/count (frames are streamed below).
    let header = StreamDecoder::open(src).map_err(|e| e.to_string())?;
    let (w, h) = (header.width(), header.height());
    let count = header.frame_count().max(1);
    let fps = header.fps();
    drop(header);

    // Read just the audio (skips frames; no full decode), write it to a temp WAV.
    let audio = read_audio_file(src).map_err(|e| e.to_string())?;
    let wav = match &audio {
        Some(track) if !track.samples.is_empty() => Some(write_temp_wav(track)?),
        _ => None,
    };

    let mut args: Vec<String> = Vec::new();
    push_args(&mut args, &["-f", "rawvideo", "-pixel_format", "rgba"]);
    args.push("-video_size".into());
    args.push(format!("{w}x{h}"));
    args.push("-framerate".into());
    args.push(format!("{}/{}", fps.num.max(1), fps.den.max(1)));
    push_args(&mut args, &["-i", "-"]); // video from stdin
    if let Some(path) = &wav {
        args.push("-i".into());
        args.push(path.display().to_string());
    }
    // Map exactly one video (input 0) and, if present, one audio (input 1).
    push_args(&mut args, &["-map", "0:v:0"]);
    if wav.is_some() {
        push_args(&mut args, &["-map", "1:a:0"]);
    }
    // yuv420p needs even dimensions — round each down to the nearest even number.
    push_args(&mut args, &["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"]);
    match format {
        ExportFormat::WebM => push_args(
            &mut args,
            &["-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "32"],
        ),
        ExportFormat::Mp4 => push_args(
            &mut args,
            &["-c:v", "libx264", "-preset", "medium", "-crf", "23"],
        ),
        ExportFormat::Gif => unreachable!(),
    }
    push_args(&mut args, &["-pix_fmt", "yuv420p"]);
    if wav.is_some() {
        match format {
            ExportFormat::WebM => push_args(&mut args, &["-c:a", "libopus"]),
            ExportFormat::Mp4 => push_args(&mut args, &["-c:a", "aac", "-b:a", "192k"]),
            ExportFormat::Gif => unreachable!(),
        }
        push_args(&mut args, &["-shortest"]);
    }
    args.push("-y".into());
    args.push(dst.display().to_string());

    let mut child = FfmpegCommand::new()
        .args(&args)
        .spawn()
        .map_err(|e| format!("Couldn't start ffmpeg: {e}"))?;
    let stdin = child
        .take_stdin()
        .ok_or_else(|| "ffmpeg did not accept piped video".to_owned())?;

    // Pipe frames on a dedicated thread so the export thread can simultaneously
    // DRAIN ffmpeg's output below. ffmpeg's stderr is piped; if it fills (~64 KB)
    // while unread, ffmpeg blocks, stops reading stdin, and the export deadlocks.
    let writer = {
        let src = src.to_path_buf();
        let tx = tx.clone();
        let ctx = ctx.clone();
        let mut stdin = stdin;
        std::thread::spawn(move || -> Result<(), String> {
            let mut decoder = StreamDecoder::open(&src).map_err(|e| e.to_string())?;
            let mut done = 0u32;
            while let Some(frame) = decoder.next_frame().map_err(|e| e.to_string())? {
                if stdin.write_all(&frame.pixels).is_err() {
                    break; // ffmpeg closed its input
                }
                done += 1;
                let _ = tx.send(ExportMsg::Progress(done as f32 / count as f32));
                ctx.request_repaint();
            }
            drop(stdin); // EOF → ffmpeg finalizes
            Ok(())
        })
    };

    // Drain ffmpeg's event/log stream to completion (this is what prevents the
    // stderr-fill deadlock); it ends when ffmpeg exits.
    if let Ok(events) = child.iter() {
        for _event in events {}
    }
    let _ = child.wait();
    let writer_result = writer
        .join()
        .unwrap_or_else(|_| Err("export writer thread panicked".to_owned()));

    if let Some(path) = wav {
        let _ = std::fs::remove_file(path);
    }

    // A failed pipe, or a missing/empty output, means the export did not succeed.
    let ok = writer_result.is_ok() && std::fs::metadata(dst).map(|m| m.len() > 0).unwrap_or(false);
    if !ok {
        let _ = std::fs::remove_file(dst);
        return writer_result.and(Err(
            "ffmpeg could not encode this recording (the format/codec \
             may be unavailable in the downloaded ffmpeg build)"
                .to_owned(),
        ));
    }
    Ok(dst.to_path_buf())
}

/// Append each string-literal arg to the ffmpeg command vector.
fn push_args(args: &mut Vec<String>, items: &[&str]) {
    args.extend(items.iter().map(|s| (*s).to_string()));
}

/// GIF output size: cap the width at [`MAX_GIF_WIDTH`], preserving aspect.
fn gif_size(w: u32, h: u32) -> (u32, u32) {
    if w <= MAX_GIF_WIDTH || w == 0 {
        (w, h)
    } else {
        let gh = ((MAX_GIF_WIDTH as u64 * h as u64) / w as u64).max(1) as u32;
        (MAX_GIF_WIDTH, gh)
    }
}

/// Write the audio track to a temporary 16-bit PCM WAV for ffmpeg to mux.
fn write_temp_wav(track: &AudioTrack) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("freally_export_{}_{seq}.wav", std::process::id()));
    let data_len = (track.samples.len() * 2) as u32;
    let byte_rate = track.sample_rate * track.channels as u32 * 2;
    let block_align = track.channels * 2;

    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    buf.extend_from_slice(&track.channels.to_le_bytes());
    buf.extend_from_slice(&track.sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for &s in &track.samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(&path, buf).map_err(|e| format!("Couldn't write temp audio: {e}"))?;
    Ok(path)
}
