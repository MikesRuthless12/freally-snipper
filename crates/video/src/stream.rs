//! Streaming `.fvid` encoder and decoder — frame-at-a-time, so an open-ended
//! recording (P5.1/P5.2) never holds the whole movie in memory.
//!
//! [`Movie::encode`](crate::Movie::encode) is a batch call: it needs every frame
//! up front and returns the whole container as one `Vec<u8>`. A screen recording
//! is open-ended and can be 4K, where a single raw frame is ~33 MB — holding them
//! all would exhaust memory. [`StreamEncoder`] instead writes the container header
//! immediately and encodes each frame as it arrives (the same intra/inter decision
//! as the batch path), so only the previous and current frame are ever in RAM.
//! Optional audio (PCM s16le, for the P5.2 A/V mux) is buffered and written as one
//! block after the frames. The frame and sample counts aren't known until the
//! recording stops, so they are written as placeholders and patched in
//! [`StreamEncoder::finish`] (hence the `Write + Seek` bound).
//!
//! The bytes produced are **identical** to [`Movie::encode`](crate::Movie::encode)
//! for the same frames and audio (verified in tests), so a streamed file also
//! decodes with [`Movie::decode`](crate::Movie::decode). [`StreamDecoder`] is the
//! matching pull reader: it reads the header, then yields one [`Frame`] at a time,
//! for playback without decoding the whole movie at once.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::bytes::Cursor;
use crate::{
    bytes_to_samples, inter, intra, pack, rgba_len, samples_to_bytes, write_block,
    write_container_header, AudioTrack, Frame, Rational, Result, VideoError, AUDIO_FORMAT_S16LE,
    AUDIO_SAMPLES_OFFSET, FLAG_AUDIO, FORMAT_VERSION, FRAME_COUNT_OFFSET, FRAME_INTER, FRAME_INTRA,
    HEADER_LEN, KEYFRAME_INTERVAL, MAGIC, MAX_BLOCK_BYTES, SCENE_CHANGE_RATIO,
};

/// Buffered interleaved PCM for the optional audio track.
struct AudioAccum {
    sample_rate: u32,
    channels: u16,
    samples: Vec<i16>,
}

/// Streaming encoder: write the header, push frames (and optionally audio) as they
/// arrive, then [`finish`](Self::finish) to patch in the final counts.
pub struct StreamEncoder<W: Write + Seek> {
    writer: W,
    width: u32,
    height: u32,
    frame_bytes: usize,
    frame_count: u32,
    /// Previous frame's pixels, reused as the delta reference (and scratch buffer)
    /// so memory stays bounded regardless of recording length.
    prev: Option<Vec<u8>>,
    /// Buffered audio, present only when the stream was opened with audio.
    audio: Option<AudioAccum>,
}

impl StreamEncoder<BufWriter<File>> {
    /// Create a video-only `.fvid` file and write its header.
    pub fn create<P: AsRef<Path>>(path: P, width: u32, height: u32, fps: Rational) -> Result<Self> {
        StreamEncoder::new(BufWriter::new(File::create(path)?), width, height, fps)
    }

    /// Create a `.fvid` file with an audio track (PCM s16le) and write its header.
    pub fn create_with_audio<P: AsRef<Path>>(
        path: P,
        width: u32,
        height: u32,
        fps: Rational,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Self> {
        StreamEncoder::new_with_audio(
            BufWriter::new(File::create(path)?),
            width,
            height,
            fps,
            sample_rate,
            channels,
        )
    }
}

impl<W: Write + Seek> StreamEncoder<W> {
    /// Start a video-only stream into `writer`, writing the header immediately.
    pub fn new(writer: W, width: u32, height: u32, fps: Rational) -> Result<Self> {
        Self::start(writer, width, height, fps, None)
    }

    /// Start a stream with an audio track. `channels` must be non-zero; the total
    /// sample count is patched in on [`finish`](Self::finish).
    pub fn new_with_audio(
        writer: W,
        width: u32,
        height: u32,
        fps: Rational,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Self> {
        if channels == 0 {
            return Err(VideoError::InvalidData("audio track has zero channels"));
        }
        Self::start(
            writer,
            width,
            height,
            fps,
            Some(AudioAccum {
                sample_rate,
                channels,
                samples: Vec::new(),
            }),
        )
    }

    /// Shared constructor: validate, write the placeholder header, build the state.
    fn start(
        mut writer: W,
        width: u32,
        height: u32,
        fps: Rational,
        audio: Option<AudioAccum>,
    ) -> Result<Self> {
        if fps.den == 0 {
            return Err(VideoError::InvalidData("fps denominator is zero"));
        }
        let frame_bytes = rgba_len(width, height)?;
        let descriptor = audio
            .as_ref()
            .map(|a| AudioTrack::new(a.sample_rate, a.channels, Vec::new()));
        let mut header = Vec::with_capacity(HEADER_LEN);
        // Frame count (and audio sample count, if any) are 0 placeholders, patched
        // in `finish`.
        write_container_header(&mut header, width, height, fps, 0, descriptor.as_ref());
        debug_assert_eq!(header.len(), HEADER_LEN);
        writer.write_all(&header)?;
        Ok(Self {
            writer,
            width,
            height,
            frame_bytes,
            frame_count: 0,
            prev: None,
            audio,
        })
    }

    /// Encode and append one frame; its dimensions must match the movie's.
    pub fn push_frame(&mut self, frame: &Frame) -> Result<()> {
        if frame.width != self.width || frame.height != self.height {
            return Err(VideoError::DimensionMismatch);
        }
        self.push_rgba(&frame.pixels)
    }

    /// Encode and append one frame from a raw RGBA8 buffer (`width*height*4` bytes).
    pub fn push_rgba(&mut self, pixels: &[u8]) -> Result<()> {
        if pixels.len() != self.frame_bytes {
            return Err(VideoError::InvalidData(
                "frame pixel buffer has wrong length",
            ));
        }
        if self.frame_count == u32::MAX {
            return Err(VideoError::InvalidData("too many frames"));
        }

        // Same intra/inter decision as the batch encoder, keyed on the frame index,
        // so the stream is byte-identical to `Movie::encode`. The inter block is
        // computed (releasing the `prev` borrow) before the writer is touched.
        let inter_block = match self.prev.as_deref() {
            Some(prev) if !self.frame_count.is_multiple_of(KEYFRAME_INTERVAL) => {
                let (delta, dirty, total) = inter::encode(prev, pixels, self.width, self.height);
                let scene_change = total > 0 && dirty as f32 >= SCENE_CHANGE_RATIO * total as f32;
                if scene_change {
                    None
                } else {
                    Some(pack::pack(&delta))
                }
            }
            _ => None,
        };
        match inter_block {
            Some(block) => write_block(&mut self.writer, FRAME_INTER, &block)?,
            None => write_block(
                &mut self.writer,
                FRAME_INTRA,
                &pack::pack(&intra::encode(pixels)),
            )?,
        }

        // Remember this frame as the next delta reference (reusing the buffer).
        match self.prev.as_mut() {
            Some(buf) => {
                buf.clear();
                buf.extend_from_slice(pixels);
            }
            None => self.prev = Some(pixels.to_vec()),
        }
        self.frame_count += 1;
        Ok(())
    }

    /// Append interleaved PCM s16le samples to the audio track. Errors if the
    /// stream was not opened with [`new_with_audio`](Self::new_with_audio).
    pub fn push_audio(&mut self, samples: &[i16]) -> Result<()> {
        match self.audio.as_mut() {
            Some(a) => {
                a.samples.extend_from_slice(samples);
                Ok(())
            }
            None => Err(VideoError::InvalidData(
                "audio was not enabled for this stream",
            )),
        }
    }

    /// Number of frames encoded so far.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Whether this stream carries an audio track.
    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// Flush, write the audio block (if any), patch the header counts, and return
    /// the underlying writer.
    pub fn finish(mut self) -> Result<W> {
        // The audio block (if present) goes after all frames.
        let audio_samples = match self.audio.take() {
            Some(audio) => {
                let mut samples = audio.samples;
                // Keep only whole interleaved sample-frames (drop a trailing partial).
                let usable = samples.len() - samples.len() % audio.channels as usize;
                samples.truncate(usable);
                let block = pack::pack(&samples_to_bytes(&samples));
                let len = u32::try_from(block.len())
                    .map_err(|_| VideoError::InvalidData("audio block too large"))?;
                self.writer.write_all(&len.to_le_bytes())?;
                self.writer.write_all(&block)?;
                Some(samples.len() as u64)
            }
            None => None,
        };

        self.writer.flush()?;
        // Patch the frame count placeholder.
        self.writer.seek(SeekFrom::Start(FRAME_COUNT_OFFSET))?;
        self.writer.write_all(&self.frame_count.to_le_bytes())?;
        // Patch the audio sample count (the rest of the descriptor was written up front).
        if let Some(n) = audio_samples {
            self.writer.seek(SeekFrom::Start(AUDIO_SAMPLES_OFFSET))?;
            self.writer.write_all(&n.to_le_bytes())?;
        }
        self.writer.seek(SeekFrom::End(0))?;
        self.writer.flush()?;
        Ok(self.writer)
    }
}

/// Pull decoder: read the header, then call [`next_frame`](Self::next_frame)
/// until it returns `None`. Decodes one [`Frame`] at a time (for playback)
/// without holding the whole movie. Any audio block follows the frames; decode
/// it with [`Movie::decode`](crate::Movie::decode) when the samples are needed.
pub struct StreamDecoder<R: Read> {
    reader: R,
    width: u32,
    height: u32,
    fps: Rational,
    frame_count: u32,
    frames_read: u32,
    has_audio: bool,
    frame_bytes: usize,
    /// Previous decoded frame, the reference for the next inter (delta) frame.
    prev: Option<Vec<u8>>,
}

impl StreamDecoder<BufReader<File>> {
    /// Open a `.fvid` file for streaming playback.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        StreamDecoder::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read> StreamDecoder<R> {
    /// Read and validate the container header from `reader`.
    pub fn new(mut reader: R) -> Result<Self> {
        let mut header = [0u8; HEADER_LEN];
        fill_exact(&mut reader, &mut header)?;
        let mut cur = Cursor::new(&header);
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
        // The audio descriptor is present in the header; the audio block itself
        // (after the frames) is not read on this streaming path.
        let _audio_rate = cur.read_u32()?;
        let _audio_channels = cur.read_u16()?;
        let _audio_format = cur.read_u8()?;
        let _audio_samples = cur.read_u64()?;
        let frame_bytes = rgba_len(width, height)?;
        Ok(Self {
            reader,
            width,
            height,
            fps,
            frame_count,
            frames_read: 0,
            has_audio: flags & FLAG_AUDIO != 0,
            frame_bytes,
            prev: None,
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

    /// Playback frame rate.
    pub fn fps(&self) -> Rational {
        self.fps
    }

    /// Total number of frames declared in the header.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Whether the container declares an audio track (not decoded on this path).
    pub fn has_audio(&self) -> bool {
        self.has_audio
    }

    /// Decode the next frame, or `None` once every frame has been read.
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        if self.frames_read >= self.frame_count {
            return Ok(None);
        }
        let mut head = [0u8; 5];
        fill_exact(&mut self.reader, &mut head)?;
        let ftype = head[0];
        let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
        if len > MAX_BLOCK_BYTES {
            return Err(VideoError::InvalidData("frame block too large"));
        }
        let mut block = vec![0u8; len];
        fill_exact(&mut self.reader, &mut block)?;
        let payload = pack::unpack(&block).ok_or(VideoError::InvalidData("corrupt block"))?;
        let pixels = match ftype {
            FRAME_INTRA => {
                let px = intra::decode(&payload, self.frame_bytes / 4)
                    .ok_or(VideoError::InvalidData("corrupt intra frame"))?;
                if px.len() != self.frame_bytes {
                    return Err(VideoError::InvalidData("intra frame has wrong size"));
                }
                px
            }
            FRAME_INTER => {
                let prev = self
                    .prev
                    .as_deref()
                    .ok_or(VideoError::InvalidData("inter frame before any keyframe"))?;
                inter::decode(&payload, prev, self.width, self.height)
                    .ok_or(VideoError::InvalidData("corrupt inter frame"))?
            }
            _ => return Err(VideoError::InvalidData("unknown frame type")),
        };
        self.prev = Some(pixels.clone());
        self.frames_read += 1;
        Ok(Some(Frame {
            width: self.width,
            height: self.height,
            pixels,
        }))
    }
}

/// Read **only** the audio track from a `.fvid` file, seeking past the frames so
/// a long/4K recording's audio can be muxed for export without decoding every
/// frame into memory. Returns `Ok(None)` when the file has no audio track.
pub fn read_audio_file<P: AsRef<Path>>(path: P) -> Result<Option<AudioTrack>> {
    let mut file = File::open(path)?;
    let mut header = [0u8; HEADER_LEN];
    fill_exact(&mut file, &mut header)?;
    let mut cur = Cursor::new(&header);
    if cur.take(4)? != MAGIC.as_slice() {
        return Err(VideoError::BadMagic);
    }
    let version = cur.read_u16()?;
    if version != FORMAT_VERSION {
        return Err(VideoError::UnsupportedVersion(version));
    }
    let flags = cur.read_u16()?;
    let _width = cur.read_u32()?;
    let _height = cur.read_u32()?;
    let _fps_num = cur.read_u32()?;
    let _fps_den = cur.read_u32()?;
    let frame_count = cur.read_u32()?;
    let audio_rate = cur.read_u32()?;
    let audio_channels = cur.read_u16()?;
    let audio_format = cur.read_u8()?;
    let audio_samples = cur.read_u64()?;

    if flags & FLAG_AUDIO == 0 {
        return Ok(None);
    }
    if audio_format != AUDIO_FORMAT_S16LE {
        return Err(VideoError::InvalidData("unsupported audio format"));
    }
    if audio_channels == 0 {
        return Err(VideoError::InvalidData("audio track has zero channels"));
    }

    // Skip each `[type][u32 len][block]` frame record via seek (no decoding).
    for _ in 0..frame_count {
        let mut block_head = [0u8; 5];
        fill_exact(&mut file, &mut block_head)?;
        let len = u32::from_le_bytes([block_head[1], block_head[2], block_head[3], block_head[4]]);
        file.seek(SeekFrom::Current(i64::from(len)))?;
    }

    // The audio block follows the frames.
    let mut len_bytes = [0u8; 4];
    fill_exact(&mut file, &mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > MAX_BLOCK_BYTES {
        return Err(VideoError::InvalidData("audio block too large"));
    }
    let mut block = vec![0u8; len];
    fill_exact(&mut file, &mut block)?;
    let pcm = pack::unpack(&block).ok_or(VideoError::InvalidData("corrupt audio block"))?;
    if !pcm.len().is_multiple_of(2) {
        return Err(VideoError::InvalidData("audio byte count is odd"));
    }
    let samples = bytes_to_samples(&pcm);
    if samples.len() as u64 != audio_samples {
        return Err(VideoError::InvalidData("audio sample count mismatch"));
    }
    Ok(Some(AudioTrack::new(audio_rate, audio_channels, samples)))
}

/// Like [`Read::read_exact`], but map an early EOF to [`VideoError::Truncated`]
/// (matching the batch decoder) rather than surfacing a generic I/O error.
fn fill_exact<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<()> {
    match reader.read_exact(buf) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Err(VideoError::Truncated),
        Err(e) => Err(VideoError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Movie;
    use std::io::Cursor as IoCursor;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Frame {
        Frame::from_rgba(w, h, &rgba.repeat((w * h) as usize)).unwrap()
    }

    /// A small clip exercising intra (frame 0), inter with a few dirty tiles,
    /// an identical (duplicate) inter frame, and a scene cut.
    fn sample_clip() -> (u32, u32, Vec<Frame>) {
        let (w, h) = (40, 30);
        let f0 = solid(w, h, [10, 20, 30, 255]);
        let mut f1 = f0.clone();
        f1.pixels[0] = 200;
        f1.pixels[4] = 150;
        let f2 = f1.clone(); // identical -> inter with zero dirty tiles (the duplicate path)
        let f3 = solid(w, h, [200, 200, 200, 255]); // scene cut -> stored as a keyframe
        (w, h, vec![f0, f1, f2, f3])
    }

    fn stream_encode(w: u32, h: u32, fps: Rational, frames: &[Frame]) -> Vec<u8> {
        let mut enc = StreamEncoder::new(IoCursor::new(Vec::new()), w, h, fps).unwrap();
        for f in frames {
            enc.push_frame(f).unwrap();
        }
        enc.finish().unwrap().into_inner()
    }

    fn stream_decode_all(bytes: Vec<u8>) -> Vec<Frame> {
        let mut dec = StreamDecoder::new(IoCursor::new(bytes)).unwrap();
        let mut out = Vec::new();
        while let Some(f) = dec.next_frame().unwrap() {
            out.push(f);
        }
        out
    }

    #[test]
    fn header_layout_matches_constants() {
        let mut h = Vec::new();
        write_container_header(&mut h, 1, 1, Rational::new(30, 1), 7, None);
        assert_eq!(h.len(), HEADER_LEN);
        let off = FRAME_COUNT_OFFSET as usize;
        assert_eq!(&h[off..off + 4], &7u32.to_le_bytes());
    }

    #[test]
    fn stream_bytes_equal_batch_encode() {
        let (w, h, frames) = sample_clip();
        let fps = Rational::new(30, 1);
        let batch = Movie::new(w, h, fps, frames.clone(), None)
            .encode()
            .unwrap();
        let streamed = stream_encode(w, h, fps, &frames);
        assert_eq!(
            streamed, batch,
            "streamed bytes must be identical to Movie::encode"
        );
    }

    #[test]
    fn stream_round_trips_and_batch_decoder_reads_it() {
        let (w, h, frames) = sample_clip();
        let bytes = stream_encode(w, h, Rational::new(30, 1), &frames);

        let dec = StreamDecoder::new(IoCursor::new(bytes.clone())).unwrap();
        assert_eq!((dec.width(), dec.height()), (w, h));
        assert_eq!(dec.frame_count(), frames.len() as u32);
        assert!(!dec.has_audio());

        assert_eq!(stream_decode_all(bytes.clone()), frames);
        assert_eq!(Movie::decode(&bytes).unwrap().frames, frames);
    }

    #[test]
    fn stream_with_audio_equals_batch_and_round_trips() {
        let (w, h, frames) = sample_clip();
        let fps = Rational::new(30, 1);
        let (rate, channels) = (48_000u32, 2u16);
        let samples: Vec<i16> = (0..2400).map(|i| ((i * 7) % 1000 - 500) as i16).collect();

        let batch = Movie::new(
            w,
            h,
            fps,
            frames.clone(),
            Some(AudioTrack::new(rate, channels, samples.clone())),
        )
        .encode()
        .unwrap();

        let mut enc =
            StreamEncoder::new_with_audio(IoCursor::new(Vec::new()), w, h, fps, rate, channels)
                .unwrap();
        assert!(enc.has_audio());
        for f in &frames {
            enc.push_frame(f).unwrap();
        }
        // Audio arrives in arbitrary chunks during a recording.
        enc.push_audio(&samples[..1000]).unwrap();
        enc.push_audio(&samples[1000..]).unwrap();
        let bytes = enc.finish().unwrap().into_inner();

        assert_eq!(bytes, batch, "streamed A/V bytes must equal Movie::encode");

        let movie = Movie::decode(&bytes).unwrap();
        assert_eq!(movie.frames, frames);
        let audio = movie.audio.expect("audio track");
        assert_eq!(audio.sample_rate, rate);
        assert_eq!(audio.channels, channels);
        assert_eq!(audio.samples, samples);
    }

    #[test]
    fn file_create_and_open_round_trip() {
        let (w, h) = (16, 16);
        let frames = vec![
            solid(w, h, [1, 2, 3, 255]),
            solid(w, h, [4, 5, 6, 255]),
            solid(w, h, [4, 5, 6, 255]),
        ];
        let path = std::env::temp_dir().join(format!(
            "freally_video_stream_round_trip_{}.fvid",
            std::process::id()
        ));
        let mut enc = StreamEncoder::create(&path, w, h, Rational::new(24, 1)).unwrap();
        for f in &frames {
            enc.push_frame(f).unwrap();
        }
        assert_eq!(enc.frame_count(), 3);
        enc.finish().unwrap();

        let mut dec = StreamDecoder::open(&path).unwrap();
        assert_eq!(dec.fps(), Rational::new(24, 1));
        let mut got = Vec::new();
        while let Some(f) = dec.next_frame().unwrap() {
            got.push(f);
        }
        assert_eq!(got, frames);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn keyframe_interval_round_trips_streaming() {
        let (w, h) = (8, 8);
        let frames: Vec<Frame> = (0..(KEYFRAME_INTERVAL + 5))
            .map(|i| solid(w, h, [(i % 256) as u8, 0, 0, 255]))
            .collect();
        let fps = Rational::new(30, 1);
        let bytes = stream_encode(w, h, fps, &frames);
        let batch = Movie::new(w, h, fps, frames.clone(), None)
            .encode()
            .unwrap();
        assert_eq!(bytes, batch);
        assert_eq!(stream_decode_all(bytes), frames);
    }

    #[test]
    fn push_rejects_dimension_mismatch() {
        let mut enc =
            StreamEncoder::new(IoCursor::new(Vec::new()), 4, 4, Rational::new(30, 1)).unwrap();
        let bad = Frame {
            width: 5,
            height: 5,
            pixels: vec![0u8; 5 * 5 * 4],
        };
        assert!(matches!(
            enc.push_frame(&bad),
            Err(VideoError::DimensionMismatch)
        ));
    }

    #[test]
    fn push_audio_without_audio_enabled_errors() {
        let mut enc =
            StreamEncoder::new(IoCursor::new(Vec::new()), 4, 4, Rational::new(30, 1)).unwrap();
        assert!(enc.push_audio(&[0i16, 1, 2]).is_err());
    }

    #[test]
    fn new_with_audio_rejects_zero_channels() {
        let r = StreamEncoder::new_with_audio(
            IoCursor::new(Vec::new()),
            4,
            4,
            Rational::new(30, 1),
            48_000,
            0,
        );
        assert!(matches!(r, Err(VideoError::InvalidData(_))));
    }

    #[test]
    fn new_rejects_zero_fps_denominator() {
        let r = StreamEncoder::new(IoCursor::new(Vec::new()), 4, 4, Rational::new(30, 0));
        assert!(matches!(r, Err(VideoError::InvalidData(_))));
    }

    #[test]
    fn read_audio_file_extracts_only_audio() {
        let (w, h, frames) = sample_clip();
        let fps = Rational::new(30, 1);
        let samples: Vec<i16> = (0..1200).map(|i| (i % 200 - 100) as i16).collect();

        let path = std::env::temp_dir().join(format!(
            "freally_video_audio_only_{}.fvid",
            std::process::id()
        ));
        let mut enc =
            StreamEncoder::create_with_audio(&path, w, h, fps, 44_100, 2).expect("encoder");
        for f in &frames {
            enc.push_frame(f).unwrap();
        }
        enc.push_audio(&samples).unwrap();
        enc.finish().unwrap();

        let audio = read_audio_file(&path).unwrap().expect("audio track");
        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.samples, samples);
        let _ = std::fs::remove_file(&path);

        // A video-only file reports no audio.
        let vpath =
            std::env::temp_dir().join(format!("freally_video_noaudio_{}.fvid", std::process::id()));
        let mut venc = StreamEncoder::create(&vpath, w, h, fps).expect("encoder");
        for f in &frames {
            venc.push_frame(f).unwrap();
        }
        venc.finish().unwrap();
        assert!(read_audio_file(&vpath).unwrap().is_none());
        let _ = std::fs::remove_file(&vpath);
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let bytes = b"NOPE plus enough trailing bytes to fill a header........".to_vec();
        assert!(matches!(
            StreamDecoder::new(IoCursor::new(bytes)),
            Err(VideoError::BadMagic)
        ));
    }

    #[test]
    fn decode_rejects_truncated_header() {
        assert!(matches!(
            StreamDecoder::new(IoCursor::new(vec![b'F', b'V', b'I', b'D'])),
            Err(VideoError::Truncated)
        ));
    }
}
