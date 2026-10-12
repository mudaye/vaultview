//! Video playback through GStreamer (`playbin`), for plain files and for
//! encrypted files.
//!
//! Encrypted video is never written to disk: a random-access, authenticating
//! [`MediaSource`] is pulled by an `appsrc` element in bounded blocks, so only
//! a few megabytes of decrypted data are in flight at any time. Decoded frames
//! are delivered as RGBA images through an `appsink`.
//!
//! Limitations: GStreamer and the platform's decoders manage their own
//! buffers, which this application cannot zeroize or keep out of swap, and
//! hardware decoders may use graphics memory.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use gstreamer_video::VideoFrameExt;
use zeroize::Zeroizing;

use crate::constants::{
    VIDEO_APPSRC_BLOCK_LEN, VIDEO_APPSRC_MAX_BYTES, VIDEO_DEFAULT_VOLUME, VIDEO_SINK_MAX_BUFFERS,
    VIDEO_STATE_TIMEOUT_SECS,
};
use crate::error::{Error, Result};
use crate::media::ImageData;

/// Anything the player can pull video bytes from.
pub trait MediaSource: Read + Seek + Send {}

impl<T: Read + Seek + Send> MediaSource for T {}

type SharedSource = Arc<Mutex<Option<Box<dyn MediaSource>>>>;
type SharedError = Arc<Mutex<Option<String>>>;

/// Initialises GStreamer. Safe to call more than once.
pub fn init() -> Result<()> {
    gst::init().map_err(|e| gst_err("GStreamer could not be initialised", e))
}

fn gst_err(context: &str, error: impl std::fmt::Display) -> Error {
    Error::Unsupported(format!("{context}: {error}"))
}

/// An in-memory media source whose buffer is zeroized when dropped.
pub struct MemorySource {
    data: Zeroizing<Vec<u8>>,
    pos: u64,
}

impl MemorySource {
    /// Wraps already-decrypted bytes.
    pub fn new(data: Zeroizing<Vec<u8>>) -> Self {
        Self { data, pos: 0 }
    }
}

impl Read for MemorySource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let start = usize::try_from(self.pos).unwrap_or(usize::MAX);
        let remaining = self.data.get(start..).unwrap_or(&[]);
        let n = remaining.len().min(out.len());
        out[..n].copy_from_slice(&remaining[..n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for MemorySource {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        let len = self.data.len() as i128;
        let (base, delta) = match target {
            SeekFrom::Start(n) => (0i128, i128::from(n)),
            SeekFrom::Current(d) => (i128::from(self.pos), i128::from(d)),
            SeekFrom::End(d) => (len, i128::from(d)),
        };
        let new = base + delta;
        if new < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the stream",
            ));
        }
        self.pos = u64::try_from(new).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "seek position out of range")
        })?;
        Ok(self.pos)
    }
}

fn read_fill<R: Read + ?Sized>(reader: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn feed(appsrc: &gst_app::AppSrc, length: u32, source: &SharedSource, error: &SharedError) {
    let want = match usize::try_from(length) {
        Ok(n) if n > 0 && n <= VIDEO_APPSRC_BLOCK_LEN => n,
        _ => VIDEO_APPSRC_BLOCK_LEN,
    };
    let mut data = vec![0u8; want];
    let result = match source.lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(src) => read_fill(&mut **src, &mut data),
            None => Ok(0),
        },
        Err(_) => Err(io::Error::other("the video source is no longer available")),
    };
    match result {
        Ok(0) => {
            let _ = appsrc.end_of_stream();
        }
        Ok(n) => {
            data.truncate(n);
            let _ = appsrc.push_buffer(gst::Buffer::from_slice(data));
        }
        Err(e) => {
            if let Ok(mut slot) = error.lock() {
                *slot = Some(e.to_string());
            }
            let _ = appsrc.end_of_stream();
        }
    }
}

fn seek_source(source: &SharedSource, offset: u64) -> bool {
    match source.lock() {
        Ok(mut guard) => guard
            .as_mut()
            .is_some_and(|src| src.seek(SeekFrom::Start(offset)).is_ok()),
        Err(_) => false,
    }
}

fn configure_appsrc(appsrc: &gst_app::AppSrc, source: SharedSource, len: u64, error: SharedError) {
    appsrc.set_stream_type(gst_app::AppStreamType::RandomAccess);
    appsrc.set_size(i64::try_from(len).unwrap_or(-1));
    appsrc.set_max_bytes(VIDEO_APPSRC_MAX_BYTES);
    let feed_source = source.clone();
    let callbacks = gst_app::AppSrcCallbacks::builder()
        .need_data(move |src, length| feed(src, length, &feed_source, &error))
        .seek_data(move |_, offset| seek_source(&source, offset))
        .build();
    appsrc.set_callbacks(callbacks);
}

fn sample_to_image(sample: &gst::Sample) -> Option<ImageData> {
    let caps = sample.caps()?;
    let info = gst_video::VideoInfo::from_caps(caps).ok()?;
    let buffer = sample.buffer()?;
    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info).ok()?;
    let width = usize::try_from(info.width()).ok()?;
    let height = usize::try_from(info.height()).ok()?;
    let stride = usize::try_from(*frame.plane_stride().first()?).ok()?;
    let data = frame.plane_data(0).ok()?;
    let row = width.checked_mul(4)?;
    let mut rgba = Vec::with_capacity(row.checked_mul(height)?);
    for y in 0..height {
        let start = y.checked_mul(stride)?;
        rgba.extend_from_slice(data.get(start..start.checked_add(row)?)?);
    }
    Some(ImageData {
        width: info.width(),
        height: info.height(),
        rgba,
    })
}

/// A running (or paused) video pipeline.
pub struct VideoPlayer {
    playbin: gst::Element,
    appsink: gst_app::AppSink,
    bus: gst::Bus,
    source: Option<SharedSource>,
    stream_error: SharedError,
    playing: bool,
    ended: bool,
    error: Option<String>,
    volume: f64,
    muted: bool,
    subtitles_on: bool,
}

impl VideoPlayer {
    /// Plays an ordinary file directly from its location.
    pub fn open_file(path: &Path) -> Result<Self> {
        let absolute = std::path::absolute(path)?;
        let uri = gst::glib::filename_to_uri(&absolute, None)
            .map_err(|e| gst_err("the file path cannot be converted to a URI", e))?;
        Self::create(uri.as_str(), None)
    }

    /// Plays from a seekable byte source (used for encrypted files).
    pub fn open_stream(mut source: Box<dyn MediaSource>) -> Result<Self> {
        let len = source.seek(SeekFrom::End(0))?;
        source.seek(SeekFrom::Start(0))?;
        Self::create("appsrc://", Some((source, len)))
    }

    fn create(uri: &str, stream: Option<(Box<dyn MediaSource>, u64)>) -> Result<Self> {
        let playbin = gst::ElementFactory::make("playbin").build().map_err(|e| {
            gst_err(
                "the GStreamer 'playbin' element is unavailable (is GStreamer installed?)",
                e,
            )
        })?;

        let appsink = gst_app::AppSink::builder().build();
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .build();
        appsink.set_caps(Some(&caps));
        appsink.set_max_buffers(VIDEO_SINK_MAX_BUFFERS);
        appsink.set_drop(true);
        appsink.set_property("sync", true);

        playbin.set_property("uri", uri);
        playbin.set_property("video-sink", &appsink);

        if let Ok(rotate) = gst::parse::bin_from_description("videoflip video-direction=auto", true)
        {
            playbin.set_property("video-filter", &rotate);
        }

        let stream_error: SharedError = Arc::new(Mutex::new(None));
        let source = match stream {
            Some((source, len)) => {
                let shared: SharedSource = Arc::new(Mutex::new(Some(source)));
                let for_signal = shared.clone();
                let error_for_signal = stream_error.clone();
                let _handler = playbin.connect("source-setup", false, move |values| {
                    let element = values.get(1)?.get::<gst::Element>().ok()?;
                    let appsrc = element.downcast::<gst_app::AppSrc>().ok()?;
                    configure_appsrc(&appsrc, for_signal.clone(), len, error_for_signal.clone());
                    None
                });
                Some(shared)
            }
            None => None,
        };

        let bus = playbin.bus().ok_or_else(|| {
            Error::Unsupported("the video pipeline has no message bus".to_string())
        })?;
        let mut player = Self {
            playbin,
            appsink,
            bus,
            source,
            stream_error,
            playing: false,
            ended: false,
            error: None,
            volume: VIDEO_DEFAULT_VOLUME,
            muted: false,
            subtitles_on: true,
        };
        player.set_volume(VIDEO_DEFAULT_VOLUME);
        player.play();
        Ok(player)
    }

    /// Starts or resumes playback (restarting from the beginning after the end).
    pub fn play(&mut self) {
        if self.ended {
            self.seek_to(0.0);
        }
        match self.playbin.set_state(gst::State::Playing) {
            Ok(_) => self.playing = true,
            Err(e) => {
                self.error
                    .get_or_insert_with(|| format!("playback could not start: {e}"));
            }
        }
    }

    /// Pauses playback.
    pub fn pause(&mut self) {
        if self.playbin.set_state(gst::State::Paused).is_ok() {
            self.playing = false;
        }
    }

    /// Toggles between playing and paused.
    pub fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Whether the video is currently playing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Whether the end of the stream was reached.
    pub fn has_ended(&self) -> bool {
        self.ended
    }

    /// The first playback error, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Current position in seconds.
    pub fn position(&self) -> Option<f64> {
        self.playbin
            .query_position::<gst::ClockTime>()
            .map(|t| t.seconds_f64())
    }

    /// Total duration in seconds, when known.
    pub fn duration(&self) -> Option<f64> {
        self.playbin
            .query_duration::<gst::ClockTime>()
            .map(|t| t.seconds_f64())
    }

    /// Seeks to an absolute position (snapping to a nearby key frame).
    pub fn seek_to(&mut self, seconds: f64) {
        let target = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        let position = gst::ClockTime::from_nseconds((target * 1_000_000_000.0) as u64);
        if self
            .playbin
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, position)
            .is_ok()
        {
            self.ended = false;
        }
    }

    /// Seeks relative to the current position.
    pub fn seek_by(&mut self, delta_seconds: f64) {
        let current = self.position().unwrap_or(0.0);
        let mut target = current + delta_seconds;
        if let Some(duration) = self.duration() {
            target = target.min(duration);
        }
        self.seek_to(target);
    }

    /// Current volume (0.0 to 1.0).
    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// Sets the volume (clamped to 0.0 to 1.0).
    pub fn set_volume(&mut self, volume: f64) {
        self.volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.playbin.set_property("volume", self.volume);
    }

    /// Whether audio is muted.
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Mutes or unmutes audio.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.playbin.set_property("mute", muted);
    }

    /// Number of audio tracks.
    pub fn audio_track_count(&self) -> i32 {
        self.playbin.property::<i32>("n-audio")
    }

    /// Index of the active audio track.
    pub fn audio_track(&self) -> i32 {
        self.playbin.property::<i32>("current-audio")
    }

    /// Selects an audio track.
    pub fn set_audio_track(&mut self, index: i32) {
        self.playbin.set_property("current-audio", index);
    }

    /// Number of subtitle tracks.
    pub fn subtitle_track_count(&self) -> i32 {
        self.playbin.property::<i32>("n-text")
    }

    /// Index of the active subtitle track.
    pub fn subtitle_track(&self) -> i32 {
        self.playbin.property::<i32>("current-text")
    }

    /// Selects a subtitle track.
    pub fn set_subtitle_track(&mut self, index: i32) {
        self.playbin.set_property("current-text", index);
    }

    /// Whether subtitles are rendered.
    pub fn subtitles_enabled(&self) -> bool {
        self.subtitles_on
    }

    /// Turns subtitle rendering on or off.
    pub fn set_subtitles_enabled(&mut self, enabled: bool) {
        let flags = if enabled {
            "audio+video+text+soft-volume"
        } else {
            "audio+video+soft-volume"
        };
        self.playbin.set_property_from_str("flags", flags);
        self.subtitles_on = enabled;
    }

    /// Processes pending pipeline messages (errors and end of stream).
    pub fn poll(&mut self) {
        if let Ok(mut slot) = self.stream_error.lock() {
            if let Some(message) = slot.take() {
                self.error = Some(message);
                self.playing = false;
            }
        }
        while let Some(message) = self
            .bus
            .pop_filtered(&[gst::MessageType::Error, gst::MessageType::Eos])
        {
            match message.view() {
                gst::MessageView::Error(err) => {
                    if self.error.is_none() {
                        self.error = Some(err.error().to_string());
                    }
                    self.playing = false;
                }
                gst::MessageView::Eos(_) => {
                    self.ended = true;
                    self.playing = false;
                }
                _ => {}
            }
        }
    }

    /// Returns the most recent decoded frame, if a new one is available.
    pub fn take_frame(&mut self) -> Option<ImageData> {
        let mut latest = None;
        while let Some(sample) = self.appsink.try_pull_sample(gst::ClockTime::ZERO) {
            latest = Some(sample);
        }
        latest.and_then(|sample| sample_to_image(&sample))
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
        let _ = self
            .playbin
            .state(gst::ClockTime::from_seconds(VIDEO_STATE_TIMEOUT_SECS));
        if let Some(source) = &self.source {
            if let Ok(mut guard) = source.lock() {
                *guard = None;
            }
        }
    }
}
