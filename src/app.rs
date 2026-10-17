//! The egui application: file browser, media viewer, encryption dialogs,
//! deletion and privacy controls.
//!
//! Long-running file operations (batch encryption, deletion) run on a worker
//! thread and report progress through a channel. Opening an image decrypts
//! and decodes on the UI thread, so very large files can briefly pause the
//! interface.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui::{self, Color32, Key, Modifiers, RichText};
use zeroize::Zeroizing;

use crate::browser::{self, Entry, EntryKind, NavContext, Selection};
use crate::constants as c;
use crate::crypto::{Decryptor, EncryptOptions};
use crate::error::{Error, Result};
use crate::favorites::{self, FavoriteStatus, Favorites};
use crate::format::KdfParams;
use crate::media::{self, Decoded, ImageData};
use crate::naming;
use crate::pdf::{self, PageStep, PdfEvent, PdfWorker};
use crate::session::Session;
use crate::shred::{self, ShredReport};
use crate::theater::{
    BossSequence, BossStep, HudTimer, LayoutState, LayoutSync, TheaterStrategy, WindowCmd,
    WindowSequencer,
};
use crate::viewer::ViewTransform;
use crate::workflow::{self, BatchReport, Outcome};

#[cfg(feature = "video")]
use crate::video::{MediaSource, MemorySource, VideoPlayer};

const GOLD: Color32 = Color32::from_rgb(230, 180, 60);
const FOLDER: Color32 = Color32::from_rgb(210, 190, 110);
const GOOD: Color32 = Color32::from_rgb(110, 200, 120);
const WARN: Color32 = Color32::from_rgb(240, 170, 60);
const BAD: Color32 = Color32::from_rgb(235, 100, 100);

/// Starts the application window and runs until it is closed.
pub fn run() -> eframe::Result<()> {
    #[cfg(feature = "video")]
    let video_ready = crate::video::init().is_ok();
    #[cfg(not(feature = "video"))]
    let video_ready = false;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(c::APP_NAME)
            .with_inner_size(c::WINDOW_DEFAULT_SIZE)
            .with_min_inner_size(c::WINDOW_MIN_SIZE),
        ..Default::default()
    };
    eframe::run_native(
        c::APP_NAME,
        options,
        Box::new(move |_cc| Ok(Box::new(VaultApp::new(video_ready)))),
    )
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct ImageView {
    path: PathBuf,
    textures: Vec<(egui::TextureHandle, Duration)>,
    index: usize,
    next_switch: f64,
    size: (f32, f32),
    original: (u32, u32),
    viewport: (f32, f32),
    transform: ViewTransform,
    encrypted: bool,
}

#[cfg(feature = "video")]
struct VideoView {
    path: PathBuf,
    player: VideoPlayer,
    texture: Option<egui::TextureHandle>,
    frame_size: (u32, u32),
    seek_preview: Option<f64>,
    encrypted: bool,
    audio_hint: bool,
    cover: Option<egui::TextureHandle>,
}

#[cfg(feature = "video")]
impl VideoView {
    /// Whether to show the audio layout (cover art and tags) instead of video frames.
    ///
    /// Before the stream has been analysed the content type decides; afterwards
    /// the number of video streams does.
    fn audio_view(&self) -> bool {
        if self.player.duration().is_some() {
            self.player.video_stream_count() == 0
        } else {
            self.audio_hint
        }
    }
}

struct ShownPage {
    texture: egui::TextureHandle,
    page: usize,
    scale_bits: u32,
}

struct PdfView {
    path: PathBuf,
    worker: PdfWorker,
    sizes: Vec<(f32, f32)>,
    page: usize,
    shown: Option<ShownPage>,
    pending: Option<(usize, u32)>,
    failed: Option<(usize, u32)>,
    page_error: Option<String>,
    degraded: bool,
    viewport: (f32, f32),
    transform: ViewTransform,
    encrypted: bool,
}

impl PdfView {
    fn new(path: PathBuf, worker: PdfWorker, encrypted: bool) -> Self {
        Self {
            path,
            worker,
            sizes: Vec::new(),
            page: 0,
            shown: None,
            pending: None,
            failed: None,
            page_error: None,
            degraded: false,
            viewport: (0.0, 0.0),
            transform: ViewTransform::new(),
            encrypted,
        }
    }

    fn loading(&self) -> bool {
        self.sizes.is_empty()
    }

    fn page_count(&self) -> usize {
        self.sizes.len()
    }

    fn size_pt(&self) -> (f32, f32) {
        self.sizes
            .get(self.page)
            .copied()
            .unwrap_or(c::PDF_DEFAULT_PAGE_SIZE)
    }

    /// Switches to `page` (clamped). Returns whether the page changed.
    fn go_to(&mut self, page: usize) -> bool {
        let page = page.min(self.sizes.len().saturating_sub(1));
        if page == self.page {
            return false;
        }
        self.page = page;
        self.shown = None;
        self.pending = None;
        self.failed = None;
        self.page_error = None;
        self.degraded = false;
        let size = self.size_pt();
        self.transform.pan_by((0.0, 0.0), size, self.viewport);
        true
    }
}

enum Content {
    Empty,
    Image(ImageView),
    Pdf(PdfView),
    #[cfg(feature = "video")]
    Video(VideoView),
    Failed {
        path: PathBuf,
        message: String,
    },
}

enum Loaded {
    Image {
        decoded: Decoded,
        encrypted: bool,
    },
    Pdf {
        worker: PdfWorker,
        encrypted: bool,
    },
    #[cfg(feature = "video")]
    Video {
        player: VideoPlayer,
        encrypted: bool,
        audio_hint: bool,
    },
}

#[cfg_attr(not(feature = "video"), allow(dead_code))]
enum VideoInput {
    Stream(Box<Decryptor<File>>),
    Memory(Zeroizing<Vec<u8>>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    Files,
    Favorites,
}

#[derive(Clone, Copy)]
enum Severity {
    Good,
    Warning,
    Bad,
}

struct Report {
    title: String,
    summary: String,
    lines: Vec<(Severity, String)>,
}

struct EncryptDialog {
    files: Vec<PathBuf>,
    password: Zeroizing<String>,
    confirm: Zeroizing<String>,
    reveal: bool,
    acknowledged: bool,
}

enum Dialog {
    Unlock {
        path: PathBuf,
        password: Zeroizing<String>,
        error: Option<String>,
        focus: bool,
    },
    Encrypt(EncryptDialog),
    Delete {
        files: Vec<PathBuf>,
        deep: bool,
        acknowledged: bool,
    },
    Report(Report),
}

impl Dialog {
    fn unlock(path: PathBuf, error: Option<&str>) -> Self {
        Dialog::Unlock {
            path,
            password: Zeroizing::new(String::new()),
            error: error.map(str::to_string),
            focus: true,
        }
    }
}

enum DialogEvent {
    None,
    Unlock(PathBuf, Zeroizing<String>),
    StartEncrypt(Vec<PathBuf>, Zeroizing<String>),
    StartDelete(Vec<PathBuf>, bool),
}

enum JobMsg {
    Progress {
        index: usize,
        total: usize,
        name: String,
    },
    Finished(JobOutcome),
}

enum JobOutcome {
    Encrypt(Result<BatchReport>),
    Delete {
        deep: bool,
        results: Vec<(PathBuf, Result<Option<ShredReport>>)>,
    },
}

struct Job {
    title: String,
    rx: Receiver<JobMsg>,
    index: usize,
    total: usize,
    current: String,
}

enum Action {
    EnterDir(PathBuf),
    GoUp,
    GoToInput,
    Refresh,
    OpenFile(PathBuf),
    ToggleSelect { index: usize, shift: bool },
    SelectAll,
    ClearSelection,
    Prev,
    Next,
    AddFavorite,
    RemoveFavorite(PathBuf),
    OpenFavorite(PathBuf),
    RecheckFavorites,
    BeginEncrypt,
    BeginDelete { deep: bool },
    CloseMedia,
    ForgetPassword,
    ClearCache,
    LockAll,
    ZoomIn,
    ZoomOut,
    ActualSize,
    FitWindow,
    FitImage,
    ResetView,
    Pan(f32, f32),
    Pdf(PageStep),
    ToggleTheater,
    ToggleHelp,
}

#[derive(Default)]
#[cfg_attr(not(feature = "video"), allow(dead_code))]
struct Keys {
    prev: bool,
    next: bool,
    zoom_in: bool,
    zoom_out: bool,
    reset: bool,
    actual: bool,
    fit_image: bool,
    theater: bool,
    pan: (f32, f32),
    page_first: bool,
    page_prev: bool,
    page_next: bool,
    page_last: bool,
    toggle: bool,
    seek_back: bool,
    seek_forward: bool,
    volume_up: bool,
    volume_down: bool,
    mute: bool,
}

/// The application state driven by eframe.
pub struct VaultApp {
    session: Session,
    favorites: Favorites,
    favorites_path: Option<PathBuf>,
    fav_status: Vec<(PathBuf, FavoriteStatus)>,
    fav_checked: f64,
    tab: LeftTab,
    dir: PathBuf,
    path_input: String,
    show_hidden: bool,
    entries: Vec<Entry>,
    listing_error: Option<String>,
    selection: Selection,
    last_checked: Option<usize>,
    nav: NavContext,
    content: Content,
    dialog: Option<Dialog>,
    job: Option<Job>,
    status: Option<(String, f64)>,
    now: f64,
    compress: bool,
    theater: bool,
    layout: LayoutSync,
    window: WindowSequencer,
    hud: HudTimer,
    boss: BossSequence,
    show_help: bool,
    texture_counter: u64,
    #[cfg_attr(not(feature = "video"), allow(dead_code))]
    video_ready: bool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(feature = "video")]
fn fmt_time(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let len = fs::metadata(path)?.len();
    if len > limit as u64 {
        return Err(Error::TooLarge {
            limit: limit as u64,
        });
    }
    Ok(fs::read(path)?)
}

#[cfg(feature = "video")]
fn load_cover(ctx: &egui::Context, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let image = match media::decode_image(bytes).ok()? {
        Decoded::Still(image) => image,
        Decoded::Animated(frames) => frames.into_iter().next()?.image,
    };
    let image = media::downscale_to_fit(image, c::AUDIO_COVER_MAX_SIDE);
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [image.width as usize, image.height as usize],
        &image.rgba,
    );
    Some(ctx.load_texture("cover-art", color, egui::TextureOptions::LINEAR))
}

fn unsupported(path: &Path) -> Error {
    Error::Unsupported(format!(
        "{} is not a supported image, video, audio file or PDF",
        file_label(path)
    ))
}

#[cfg(windows)]
fn roots() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|letter| PathBuf::from(format!("{}:\\", char::from(letter))))
        .filter(|p| p.exists())
        .collect()
}

#[cfg(not(windows))]
fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

fn kind_style(kind: EntryKind) -> (&'static str, Color32) {
    match kind {
        EntryKind::Directory => ("📁", FOLDER),
        EntryKind::Image => ("🖼", Color32::LIGHT_GRAY),
        EntryKind::Video => ("🎬", Color32::LIGHT_GRAY),
        EntryKind::Audio => ("🎵", Color32::LIGHT_GRAY),
        EntryKind::Pdf => ("📕", Color32::LIGHT_GRAY),
        EntryKind::Encrypted => ("🔒", GOLD),
        EntryKind::Other => ("📄", Color32::GRAY),
    }
}

fn severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Good => GOOD,
        Severity::Warning => WARN,
        Severity::Bad => BAD,
    }
}

fn paint_texture(
    ui: &egui::Ui,
    rect: egui::Rect,
    texture: &egui::TextureHandle,
    transform: &ViewTransform,
    size: (f32, f32),
) {
    let painter = ui.painter_at(rect);
    let (x, y, w, h) = transform.image_rect(size, (rect.width(), rect.height()));
    let target = egui::Rect::from_min_size(rect.min + egui::vec2(x, y), egui::vec2(w, h));
    painter.image(
        texture.id(),
        target,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

// ---------------------------------------------------------------------------
// Application logic
// ---------------------------------------------------------------------------

impl VaultApp {
    fn new(video_ready: bool) -> Self {
        let favorites_path = favorites::default_store_path();
        let mut startup_note = None;
        let favorites = match &favorites_path {
            Some(path) => match Favorites::load(path) {
                Ok(list) => list,
                Err(error) => {
                    startup_note = Some(format!(
                        "Favorites could not be loaded ({error}); starting with an empty list."
                    ));
                    Favorites::new()
                }
            },
            None => Favorites::new(),
        };
        let dir = dirs::picture_dir()
            .filter(|p| p.is_dir())
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut app = Self {
            session: Session::new(),
            favorites,
            favorites_path,
            fav_status: Vec::new(),
            fav_checked: f64::NEG_INFINITY,
            tab: LeftTab::Files,
            dir: PathBuf::from("."),
            path_input: String::new(),
            show_hidden: c::SHOW_HIDDEN_DEFAULT,
            entries: Vec::new(),
            listing_error: None,
            selection: Selection::default(),
            last_checked: None,
            nav: NavContext::default(),
            content: Content::Empty,
            dialog: None,
            job: None,
            status: None,
            now: 0.0,
            compress: c::COMPRESS_ON_ENCRYPT_DEFAULT,
            theater: false,
            layout: LayoutSync::default(),
            window: WindowSequencer::new(
                TheaterStrategy::from_setting(
                    std::env::var(c::THEATER_STRATEGY_ENV).ok().as_deref(),
                ),
                c::THEATER_UNMAXIMIZE_TIMEOUT_FRAMES,
            ),
            hud: HudTimer::default(),
            boss: BossSequence::default(),
            show_help: false,
            texture_counter: 0,
            video_ready,
        };
        app.set_dir(dir);
        if let Some(note) = startup_note {
            app.say(note);
        }
        app
    }

    fn say(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), self.now + c::STATUS_MESSAGE_SECS));
    }

    fn set_dir(&mut self, dir: PathBuf) {
        match browser::list_directory(&dir, self.show_hidden) {
            Ok(entries) => {
                self.entries = entries;
                self.dir = dir;
                self.listing_error = None;
                self.path_input = self.dir.display().to_string();
                self.selection.clear();
                self.last_checked = None;
                self.nav = NavContext::from_entries(&self.entries);
                if let Some(open) = self.content_path() {
                    self.nav.set_current(&open);
                }
            }
            Err(error) => {
                self.listing_error = Some(format!("Cannot open {}: {error}", dir.display()));
                self.path_input = self.dir.display().to_string();
            }
        }
    }

    fn content_path(&self) -> Option<PathBuf> {
        match &self.content {
            Content::Empty => None,
            Content::Image(view) => Some(view.path.clone()),
            Content::Pdf(view) => Some(view.path.clone()),
            #[cfg(feature = "video")]
            Content::Video(view) => Some(view.path.clone()),
            Content::Failed { path, .. } => Some(path.clone()),
        }
    }

    fn open_file_info(&self) -> Option<(PathBuf, bool)> {
        match &self.content {
            Content::Image(view) => Some((view.path.clone(), view.encrypted)),
            Content::Pdf(view) => Some((view.path.clone(), view.encrypted)),
            #[cfg(feature = "video")]
            Content::Video(view) => Some((view.path.clone(), view.encrypted)),
            _ => None,
        }
    }

    fn close_media(&mut self) {
        self.content = Content::Empty;
    }

    fn save_favorites(&mut self) {
        let Some(path) = self.favorites_path.clone() else {
            return;
        };
        if let Err(error) = self.favorites.save(&path) {
            self.say(format!("Could not save favorites: {error}"));
        }
    }

    fn refresh_favorite_status(&mut self) {
        self.fav_status = self
            .favorites
            .folders()
            .iter()
            .map(|folder| (folder.clone(), favorites::status(folder)))
            .collect();
        self.fav_checked = self.now;
    }

    // ----- loading and showing media -----

    fn open_media(&mut self, ctx: &egui::Context, path: PathBuf, password: Option<&str>) {
        self.close_media();
        match self.load(&path, password) {
            Ok(loaded) => self.install(ctx, path, loaded),
            Err(Error::PasswordRequired) => self.dialog = Some(Dialog::unlock(path, None)),
            Err(Error::WrongPassword) => {
                self.dialog = Some(Dialog::unlock(
                    path,
                    Some("Wrong password, or the file's key header was modified."),
                ));
            }
            Err(error) => self.fail(path, error),
        }
    }

    fn fail(&mut self, path: PathBuf, error: Error) {
        self.nav.set_current(&path);
        self.content = Content::Failed {
            message: format!("{}: {error}", file_label(&path)),
            path,
        };
    }

    fn load(&mut self, path: &Path, password: Option<&str>) -> Result<Loaded> {
        match browser::classify_file(path)? {
            EntryKind::Image => {
                let bytes = read_limited(path, c::MAX_DECRYPTED_IMAGE_BYTES)?;
                Ok(Loaded::Image {
                    decoded: media::decode_image(&bytes)?,
                    encrypted: false,
                })
            }
            EntryKind::Pdf => {
                let bytes = read_limited(path, c::MAX_PDF_BYTES)?;
                Ok(Loaded::Pdf {
                    worker: PdfWorker::open(bytes),
                    encrypted: false,
                })
            }
            EntryKind::Video => self.load_video_file(path, false),
            EntryKind::Audio => self.load_video_file(path, true),
            EntryKind::Encrypted => self.load_encrypted(path, password),
            EntryKind::Directory | EntryKind::Other => Err(unsupported(path)),
        }
    }

    fn load_encrypted(&mut self, path: &Path, password: Option<&str>) -> Result<Loaded> {
        if let Some(bytes) = self.session.cache.get(path) {
            let decoded = media::decode_image(bytes)?;
            return Ok(Loaded::Image {
                decoded,
                encrypted: true,
            });
        }
        let mut decryptor = self.session.unlock(path, password)?;
        let extension = naming::original_path(path)
            .and_then(|p| p.extension().map(|e| e.to_string_lossy().into_owned()));

        if decryptor.is_compressed() {
            let data =
                decryptor.read_plaintext(c::MAX_DECRYPTED_IMAGE_BYTES.max(c::MAX_PDF_BYTES))?;
            let kind = browser::classify_bytes(
                &data[..data.len().min(c::SNIFF_LEN)],
                extension.as_deref(),
            );
            match kind {
                EntryKind::Image => self.finish_image(path, &data),
                EntryKind::Pdf => Ok(Self::finish_pdf(data)),
                EntryKind::Video => self.load_video_decrypted(VideoInput::Memory(data), false),
                EntryKind::Audio => self.load_video_decrypted(VideoInput::Memory(data), true),
                _ => Err(unsupported(path)),
            }
        } else {
            let mut prefix = Vec::with_capacity(c::SNIFF_LEN);
            decryptor.seek(SeekFrom::Start(0))?;
            decryptor
                .by_ref()
                .take(c::SNIFF_LEN as u64)
                .read_to_end(&mut prefix)?;
            let kind = browser::classify_bytes(&prefix, extension.as_deref());
            match kind {
                EntryKind::Image => {
                    let data = decryptor.read_plaintext(c::MAX_DECRYPTED_IMAGE_BYTES)?;
                    self.finish_image(path, &data)
                }
                EntryKind::Pdf => {
                    let data = decryptor.read_plaintext(c::MAX_PDF_BYTES)?;
                    Ok(Self::finish_pdf(data))
                }
                EntryKind::Video => {
                    self.load_video_decrypted(VideoInput::Stream(Box::new(decryptor)), false)
                }
                EntryKind::Audio => {
                    self.load_video_decrypted(VideoInput::Stream(Box::new(decryptor)), true)
                }
                _ => Err(unsupported(path)),
            }
        }
    }

    fn finish_image(&mut self, path: &Path, data: &[u8]) -> Result<Loaded> {
        let decoded = media::decode_image(data)?;
        self.session.cache.insert(path.to_path_buf(), data.to_vec());
        Ok(Loaded::Image {
            decoded,
            encrypted: true,
        })
    }

    /// Hands decrypted PDF bytes to a render worker. The bytes are moved, not
    /// copied; the PDF parser keeps its own copy, which is not zeroized.
    fn finish_pdf(mut data: Zeroizing<Vec<u8>>) -> Loaded {
        Loaded::Pdf {
            worker: PdfWorker::open(std::mem::take(&mut *data)),
            encrypted: true,
        }
    }

    #[cfg(feature = "video")]
    fn require_video(&self) -> Result<()> {
        if self.video_ready {
            Ok(())
        } else {
            Err(Error::Unsupported(
                "video playback is unavailable because GStreamer could not be initialised"
                    .to_string(),
            ))
        }
    }

    #[cfg(feature = "video")]
    fn load_video_file(&self, path: &Path, audio_hint: bool) -> Result<Loaded> {
        self.require_video()?;
        Ok(Loaded::Video {
            player: VideoPlayer::open_file(path)?,
            encrypted: false,
            audio_hint,
        })
    }

    #[cfg(feature = "video")]
    fn load_video_decrypted(&self, input: VideoInput, audio_hint: bool) -> Result<Loaded> {
        self.require_video()?;
        let source: Box<dyn MediaSource> = match input {
            VideoInput::Stream(decryptor) => decryptor,
            VideoInput::Memory(data) => Box::new(MemorySource::new(data)),
        };
        Ok(Loaded::Video {
            player: VideoPlayer::open_stream(source)?,
            encrypted: true,
            audio_hint,
        })
    }

    #[cfg(not(feature = "video"))]
    fn load_video_file(&self, _path: &Path, _audio_hint: bool) -> Result<Loaded> {
        Err(Error::Unsupported(
            "this build of the application has no video or audio support".to_string(),
        ))
    }

    #[cfg(not(feature = "video"))]
    fn load_video_decrypted(&self, _input: VideoInput, _audio_hint: bool) -> Result<Loaded> {
        Err(Error::Unsupported(
            "this build of the application has no video or audio support".to_string(),
        ))
    }

    fn upload(&mut self, ctx: &egui::Context, image: &ImageData) -> egui::TextureHandle {
        self.texture_counter += 1;
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [image.width as usize, image.height as usize],
            &image.rgba,
        );
        ctx.load_texture(
            format!("media-{}", self.texture_counter),
            color,
            egui::TextureOptions::LINEAR,
        )
    }

    fn install(&mut self, ctx: &egui::Context, path: PathBuf, loaded: Loaded) {
        self.nav.set_current(&path);
        self.content = match loaded {
            Loaded::Image { decoded, encrypted } => {
                Content::Image(self.make_image_view(ctx, path, decoded, encrypted))
            }
            Loaded::Pdf { worker, encrypted } => {
                Content::Pdf(PdfView::new(path, worker, encrypted))
            }
            #[cfg(feature = "video")]
            Loaded::Video {
                player,
                encrypted,
                audio_hint,
            } => Content::Video(VideoView {
                path,
                player,
                texture: None,
                frame_size: (0, 0),
                seek_preview: None,
                encrypted,
                audio_hint,
                cover: None,
            }),
        };
    }

    fn make_image_view(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        decoded: Decoded,
        encrypted: bool,
    ) -> ImageView {
        let frames: Vec<(ImageData, Duration)> = match decoded {
            Decoded::Still(image) => vec![(image, Duration::ZERO)],
            Decoded::Animated(frames) => frames.into_iter().map(|f| (f.image, f.delay)).collect(),
        };
        let original = frames
            .first()
            .map(|(i, _)| (i.width, i.height))
            .unwrap_or((1, 1));
        let mut shown = (1u32, 1u32);
        let mut textures = Vec::with_capacity(frames.len());
        for (index, (image, delay)) in frames.into_iter().enumerate() {
            let image = media::downscale_to_fit(image, c::MAX_TEXTURE_SIDE);
            if index == 0 {
                shown = (image.width, image.height);
            }
            textures.push((self.upload(ctx, &image), delay));
        }
        let first_delay = textures.first().map(|(_, d)| *d).unwrap_or(Duration::ZERO);
        ImageView {
            path,
            textures,
            index: 0,
            next_switch: self.now + first_delay.as_secs_f64(),
            size: (shown.0 as f32, shown.1 as f32),
            original,
            viewport: (1.0, 1.0),
            transform: ViewTransform::new(),
            encrypted,
        }
    }

    fn navigate(&mut self, ctx: &egui::Context, forward: bool) {
        let target: Option<PathBuf> = if self.nav.current().is_some() {
            (if forward {
                self.nav.next_path()
            } else {
                self.nav.previous_path()
            })
            .map(Path::to_path_buf)
        } else {
            let media: Vec<&Entry> = self
                .entries
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind,
                        EntryKind::Image
                            | EntryKind::Video
                            | EntryKind::Audio
                            | EntryKind::Pdf
                            | EntryKind::Encrypted
                    )
                })
                .collect();
            (if forward { media.first() } else { media.last() }).map(|e| e.path.clone())
        };
        match target {
            Some(path) => self.open_media(ctx, path, None),
            None => self.say(if forward {
                "This is the last viewable file in the folder."
            } else {
                "This is the first viewable file in the folder."
            }),
        }
    }

    /// Runs `f` with the zoom/pan state, image size and viewport size of the
    /// open image or PDF page (does nothing for other content).
    fn with_view(&mut self, f: impl FnOnce(&mut ViewTransform, (f32, f32), (f32, f32))) {
        match &mut self.content {
            Content::Image(view) => f(&mut view.transform, view.size, view.viewport),
            Content::Pdf(view) => {
                let size = view.size_pt();
                f(&mut view.transform, size, view.viewport);
            }
            _ => {}
        }
    }

    /// Moves to another page of the open PDF.
    fn pdf_navigate(&mut self, nav: PageStep) {
        if let Content::Pdf(view) = &mut self.content {
            let target = pdf::target_page(view.page, view.page_count(), nav);
            view.go_to(target);
        }
    }

    // ----- encryption, deletion and background jobs -----

    fn begin_encrypt(&mut self) {
        let mut files = browser::encryptable_selection(&self.selection, &self.entries);
        if files.is_empty() {
            if let Some((path, false)) = self.open_file_info() {
                files.push(path);
            }
        }
        if files.is_empty() {
            self.say("Tick one or more unencrypted files in the list first (or open one).");
            return;
        }
        self.dialog = Some(Dialog::Encrypt(EncryptDialog {
            files,
            password: Zeroizing::new(String::new()),
            confirm: Zeroizing::new(String::new()),
            reveal: false,
            acknowledged: false,
        }));
    }

    fn begin_delete(&mut self, deep: bool) {
        let mut files = self.selection.paths();
        if files.is_empty() {
            if let Some((path, _)) = self.open_file_info() {
                files.push(path);
            }
        }
        if files.is_empty() {
            self.say("Tick one or more files in the list first (or open one).");
            return;
        }
        self.dialog = Some(Dialog::Delete {
            files,
            deep,
            acknowledged: false,
        });
    }

    fn spawn_encrypt(
        &mut self,
        ctx: &egui::Context,
        files: Vec<PathBuf>,
        password: Zeroizing<String>,
    ) {
        self.close_media();
        self.session.passwords.remember(&password);
        let options = EncryptOptions {
            kdf: KdfParams::default(),
            compress: self.compress,
        };
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        let total = files.len();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress_repaint = repaint.clone();
            let result =
                workflow::encrypt_batch(&files, &password, &options, &mut |index, total, path| {
                    let _ = progress_tx.send(JobMsg::Progress {
                        index,
                        total,
                        name: file_label(path),
                    });
                    progress_repaint.request_repaint();
                });
            let _ = tx.send(JobMsg::Finished(JobOutcome::Encrypt(result)));
            repaint.request_repaint();
        });
        self.job = Some(Job {
            title: "Encrypting".to_string(),
            rx,
            index: 0,
            total,
            current: String::new(),
        });
    }

    fn spawn_delete(&mut self, ctx: &egui::Context, files: Vec<PathBuf>, deep: bool) {
        self.close_media();
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        let total = files.len();
        std::thread::spawn(move || {
            let mut results = Vec::with_capacity(files.len());
            for (index, path) in files.into_iter().enumerate() {
                let _ = tx.send(JobMsg::Progress {
                    index,
                    total,
                    name: file_label(&path),
                });
                repaint.request_repaint();
                let result = if deep {
                    shred::deep_delete(&path).map(Some)
                } else {
                    shred::normal_delete(&path).map(|()| None)
                };
                results.push((path, result));
            }
            let _ = tx.send(JobMsg::Finished(JobOutcome::Delete { deep, results }));
            repaint.request_repaint();
        });
        let title = if deep { "Deep deleting" } else { "Deleting" };
        self.job = Some(Job {
            title: title.to_string(),
            rx,
            index: 0,
            total,
            current: String::new(),
        });
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.job else {
            return;
        };
        let mut finished = None;
        let mut disconnected = false;
        loop {
            match job.rx.try_recv() {
                Ok(JobMsg::Progress { index, total, name }) => {
                    job.index = index;
                    job.total = total;
                    job.current = name;
                }
                Ok(JobMsg::Finished(outcome)) => {
                    finished = Some(outcome);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if let Some(outcome) = finished {
            self.job = None;
            self.finish_job(outcome);
        } else if disconnected {
            self.job = None;
            self.say("The background operation ended unexpectedly. Refresh the folder and check your files.");
            self.refresh_listing();
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn refresh_listing(&mut self) {
        let dir = self.dir.clone();
        self.set_dir(dir);
    }

    fn finish_job(&mut self, outcome: JobOutcome) {
        let report = match outcome {
            JobOutcome::Encrypt(Ok(batch)) => {
                let lines = batch
                    .files
                    .iter()
                    .map(|file| {
                        let severity = match file.outcome {
                            Outcome::Encrypted { .. } => Severity::Good,
                            Outcome::Failed { .. } => Severity::Bad,
                            Outcome::Indeterminate { .. } => Severity::Warning,
                        };
                        (severity, file.message())
                    })
                    .collect();
                let mut summary = format!(
                    "{} encrypted, {} failed, {} uncertain.",
                    batch.succeeded(),
                    batch.failed(),
                    batch.indeterminate()
                );
                if batch.succeeded() > 0 {
                    summary.push_str(
                        " Keep the password safe: without it these files cannot be recovered.",
                    );
                }
                Report {
                    title: "Encryption finished".to_string(),
                    summary,
                    lines,
                }
            }
            JobOutcome::Encrypt(Err(error)) => Report {
                title: "Encryption failed".to_string(),
                summary: "No file was changed.".to_string(),
                lines: vec![(Severity::Bad, error.to_string())],
            },
            JobOutcome::Delete { deep, results } => {
                let mut ok = 0usize;
                let mut lines = Vec::new();
                for (path, result) in &results {
                    match result {
                        Ok(Some(done)) => {
                            ok += 1;
                            lines.push((
                                Severity::Good,
                                format!(
                                    "Deep-deleted {} ({} overwrite passes, then deletion requested). Physical erasure is not guaranteed.",
                                    path.display(),
                                    done.passes
                                ),
                            ));
                        }
                        Ok(None) => {
                            ok += 1;
                            lines.push((Severity::Good, format!("Deleted {}", path.display())));
                        }
                        Err(error) => lines.push((
                            Severity::Bad,
                            format!(
                                "Failed: {}: {error}. The file may still exist{}.",
                                path.display(),
                                if deep {
                                    " and may have been partially overwritten"
                                } else {
                                    ""
                                }
                            ),
                        )),
                    }
                }
                Report {
                    title: if deep {
                        "Deep deletion finished"
                    } else {
                        "Deletion finished"
                    }
                    .to_string(),
                    summary: format!("{ok} of {} file(s) processed successfully.", results.len()),
                    lines,
                }
            }
        };
        self.dialog = Some(Dialog::Report(report));
        self.refresh_listing();
    }

    // ----- actions -----

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::EnterDir(dir) => {
                self.tab = LeftTab::Files;
                self.set_dir(dir);
            }
            Action::GoUp => {
                if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
                    self.set_dir(parent);
                }
            }
            Action::GoToInput => {
                let target = PathBuf::from(self.path_input.trim());
                self.set_dir(target);
            }
            Action::Refresh => self.refresh_listing(),
            Action::OpenFile(path) => self.open_media(ctx, path, None),
            Action::ToggleSelect { index, shift } => {
                match (shift, self.last_checked) {
                    (true, Some(last)) => self.selection.select_range(&self.entries, last, index),
                    _ => {
                        if let Some(entry) = self.entries.get(index) {
                            self.selection.toggle(entry);
                        }
                    }
                }
                self.last_checked = Some(index);
            }
            Action::SelectAll => self.selection.select_all_files(&self.entries),
            Action::ClearSelection => self.selection.clear(),
            Action::Prev => self.navigate(ctx, false),
            Action::Next => self.navigate(ctx, true),
            Action::AddFavorite => {
                let folder = self.dir.clone();
                match self.favorites.add(&folder) {
                    Ok(true) => {
                        self.save_favorites();
                        self.refresh_favorite_status();
                        self.say(format!("Added {} to favorites.", folder.display()));
                    }
                    Ok(false) => self.say("This folder is already a favorite."),
                    Err(error) => self.say(format!("Could not add favorite: {error}")),
                }
            }
            Action::RemoveFavorite(folder) => {
                if self.favorites.remove(&folder) {
                    self.save_favorites();
                    self.refresh_favorite_status();
                }
            }
            Action::OpenFavorite(folder) => {
                self.tab = LeftTab::Files;
                self.set_dir(folder);
            }
            Action::RecheckFavorites => self.refresh_favorite_status(),
            Action::BeginEncrypt => self.begin_encrypt(),
            Action::BeginDelete { deep } => self.begin_delete(deep),
            Action::CloseMedia => {
                self.close_media();
                self.say("Media closed; its buffers were released.");
            }
            Action::ForgetPassword => {
                self.session.forget_password();
                self.say("Session passwords forgotten.");
            }
            Action::ClearCache => {
                self.session.clear_cache();
                self.say("Decrypted-content cache cleared. Close the open file to release it too.");
            }
            Action::LockAll => {
                self.close_media();
                self.session.shutdown();
                self.say("Closed media, forgot passwords and cleared the decrypted cache.");
            }
            Action::ZoomIn => self.with_view(|t, size, viewport| t.zoom_in(size, viewport)),
            Action::ZoomOut => self.with_view(|t, size, viewport| t.zoom_out(size, viewport)),
            Action::ActualSize => self.with_view(|t, _, _| t.actual_size()),
            Action::FitWindow => self.with_view(|t, _, _| t.fit_window()),
            Action::FitImage => self.with_view(|t, _, _| t.fit_image()),
            Action::ResetView => self.with_view(|t, _, _| t.reset()),
            Action::Pan(dx, dy) => {
                self.with_view(|t, size, viewport| t.pan_by((dx, dy), size, viewport))
            }
            Action::Pdf(nav) => self.pdf_navigate(nav),
            Action::ToggleTheater => {
                self.theater = !self.theater;
                if self.theater {
                    self.hud.poke(self.now);
                }
                let on = self.theater;
                self.set_theater_window(ctx, on);
            }
            Action::ToggleHelp => self.show_help = !self.show_help,
        }
    }

    // ----- keyboard -----

    fn read_keys(&self, ctx: &egui::Context) -> Keys {
        let mut keys = Keys::default();
        if ctx.wants_keyboard_input() || self.dialog.is_some() || self.job.is_some() {
            return keys;
        }
        ctx.input_mut(|input| {
            let mut pressed = |key: Key| input.consume_key(Modifiers::NONE, key);
            keys.prev = pressed(Key::ArrowLeft);
            keys.next = pressed(Key::ArrowRight);
            keys.zoom_in = pressed(Key::Plus) | pressed(Key::Equals);
            keys.zoom_out = pressed(Key::Minus);
            keys.reset = pressed(Key::Num0);
            keys.actual = pressed(Key::Num1);
            keys.fit_image = pressed(Key::Num2);
            keys.theater = pressed(Key::F) | pressed(Key::F11);
            keys.page_first = pressed(Key::Home);
            keys.page_prev = pressed(Key::PageUp);
            keys.page_next = pressed(Key::PageDown);
            keys.page_last = pressed(Key::End);
            keys.toggle = pressed(Key::Space) | pressed(Key::K);
            keys.seek_back = pressed(Key::J);
            keys.seek_forward = pressed(Key::L);
            keys.volume_up = pressed(Key::ArrowUp);
            keys.volume_down = pressed(Key::ArrowDown);
            keys.mute = pressed(Key::M);
            if pressed(Key::A) {
                keys.pan.0 += c::PAN_KEY_STEP_PX;
            }
            if pressed(Key::D) {
                keys.pan.0 -= c::PAN_KEY_STEP_PX;
            }
            if pressed(Key::W) {
                keys.pan.1 += c::PAN_KEY_STEP_PX;
            }
            if pressed(Key::S) {
                keys.pan.1 -= c::PAN_KEY_STEP_PX;
            }
        });
        keys
    }

    fn apply_keys(&mut self, ctx: &egui::Context, keys: Keys) {
        let mut actions = Vec::new();
        if keys.prev {
            actions.push(Action::Prev);
        }
        if keys.next {
            actions.push(Action::Next);
        }
        if keys.zoom_in {
            actions.push(Action::ZoomIn);
        }
        if keys.zoom_out {
            actions.push(Action::ZoomOut);
        }
        if keys.reset {
            actions.push(Action::ResetView);
        }
        if keys.actual {
            actions.push(Action::ActualSize);
        }
        if keys.fit_image {
            actions.push(Action::FitImage);
        }
        if keys.theater {
            actions.push(Action::ToggleTheater);
        }
        if keys.page_first {
            actions.push(Action::Pdf(PageStep::First));
        }
        if keys.page_prev {
            actions.push(Action::Pdf(PageStep::Prev));
        }
        if keys.page_next {
            actions.push(Action::Pdf(PageStep::Next));
        }
        if keys.page_last {
            actions.push(Action::Pdf(PageStep::Last));
        }
        if keys.pan != (0.0, 0.0) {
            actions.push(Action::Pan(keys.pan.0, keys.pan.1));
        }
        for action in actions {
            self.apply(ctx, action);
        }
        #[cfg(feature = "video")]
        if let Content::Video(view) = &mut self.content {
            if keys.toggle {
                view.player.toggle();
            }
            if keys.seek_back {
                view.player.seek_by(-c::VIDEO_SEEK_STEP_SECS);
            }
            if keys.seek_forward {
                view.player.seek_by(c::VIDEO_SEEK_STEP_SECS);
            }
            if keys.volume_up {
                let volume = view.player.volume() + c::VIDEO_VOLUME_STEP;
                view.player.set_volume(volume);
            }
            if keys.volume_down {
                let volume = view.player.volume() - c::VIDEO_VOLUME_STEP;
                view.player.set_volume(volume);
            }
            if keys.mute {
                let muted = !view.player.is_muted();
                view.player.set_muted(muted);
            }
        }
    }

    // ----- per-frame media upkeep -----

    /// Applies worker results to an open PDF and asks for the page to be
    /// (re-)rendered when the page or zoom changed. Returns a message when the
    /// whole document has to be given up.
    fn tick_pdf(view: &mut PdfView, ctx: &egui::Context) -> Option<String> {
        let mut failure = None;
        for event in view.worker.poll() {
            match event {
                PdfEvent::Opened { sizes } => view.sizes = sizes,
                PdfEvent::OpenFailed(message) => failure = Some(message),
                PdfEvent::Crashed => {
                    failure = Some("the PDF renderer stopped unexpectedly".to_string());
                }
                PdfEvent::Rendered {
                    page,
                    scale,
                    image,
                    degraded,
                } => {
                    if view.pending == Some((page, scale.to_bits())) {
                        view.pending = None;
                    }
                    if page == view.page {
                        let color = egui::ColorImage::from_rgba_unmultiplied(
                            [image.width as usize, image.height as usize],
                            &image.rgba,
                        );
                        let texture =
                            ctx.load_texture("pdf-page", color, egui::TextureOptions::LINEAR);
                        view.shown = Some(ShownPage {
                            texture,
                            page,
                            scale_bits: scale.to_bits(),
                        });
                        view.degraded = degraded;
                        view.page_error = None;
                    }
                }
                PdfEvent::RenderFailed {
                    page,
                    scale,
                    message,
                } => {
                    if view.pending == Some((page, scale.to_bits())) {
                        view.pending = None;
                    }
                    if page == view.page {
                        view.failed = Some((page, scale.to_bits()));
                        view.page_error = Some(message);
                        view.shown = None;
                    }
                }
            }
        }
        if failure.is_some() {
            return failure;
        }
        if !view.loading() && view.viewport.0 >= 2.0 && view.viewport.1 >= 2.0 {
            let size = view.size_pt();
            let scale = pdf::render_scale(
                view.transform.scale(size, view.viewport),
                ctx.pixels_per_point(),
                size,
            );
            let want = (view.page, scale.to_bits());
            let have = view.shown.as_ref().map(|s| (s.page, s.scale_bits));
            if Some(want) != have && Some(want) != view.pending && Some(want) != view.failed {
                view.worker.request(view.page, scale);
                view.pending = Some(want);
            }
        }
        if view.loading() || view.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(c::PDF_POLL_MS));
        }
        None
    }

    fn tick_media(&mut self, ctx: &egui::Context) {
        let now = self.now;
        let mut failure: Option<(PathBuf, String)> = None;
        match &mut self.content {
            Content::Image(view) if view.textures.len() > 1 => {
                if now >= view.next_switch {
                    view.index = (view.index + 1) % view.textures.len();
                    let delay = view.textures[view.index].1;
                    view.next_switch = now + delay.as_secs_f64();
                }
                let wait = (view.next_switch - now).max(0.0);
                ctx.request_repaint_after(Duration::from_secs_f64(wait));
            }
            #[cfg(feature = "video")]
            Content::Video(view) => {
                view.player.poll();
                if let Some(frame) = view.player.take_frame() {
                    let dims = (frame.width, frame.height);
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width as usize, frame.height as usize],
                        &frame.rgba,
                    );
                    if view.texture.is_some() && view.frame_size == dims {
                        if let Some(texture) = view.texture.as_mut() {
                            texture.set(color, egui::TextureOptions::LINEAR);
                        }
                    } else {
                        view.texture = Some(ctx.load_texture(
                            "video-frame",
                            color,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    view.frame_size = dims;
                }
                if view.cover.is_none() {
                    if let Some(bytes) = view.player.take_cover_art() {
                        view.cover = load_cover(ctx, &bytes);
                    }
                }
                let interval = if view.player.is_playing() {
                    c::VIDEO_REPAINT_MS
                } else {
                    c::VIDEO_REPAINT_MS * 8
                };
                ctx.request_repaint_after(Duration::from_millis(interval));
            }
            Content::Pdf(view) => {
                if let Some(message) = Self::tick_pdf(view, ctx) {
                    failure = Some((view.path.clone(), message));
                }
            }
            _ => {}
        }
        if let Some((path, message)) = failure {
            self.content = Content::Failed {
                message: format!("{}: {message}", file_label(&path)),
                path,
            };
        }
    }

    // ----- drawing -----

    fn ui_toolbar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let busy = self.job.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                if ui
                    .button("Encrypt…")
                    .on_hover_text("Encrypt the ticked files (or the open file) in place")
                    .clicked()
                {
                    actions.push(Action::BeginEncrypt);
                }
                if ui
                    .button("Delete…")
                    .on_hover_text("Delete using the system's standard file deletion")
                    .clicked()
                {
                    actions.push(Action::BeginDelete { deep: false });
                }
                if ui
                    .button(RichText::new("Deep delete…").color(BAD))
                    .on_hover_text("Overwrite with random data, then delete")
                    .clicked()
                {
                    actions.push(Action::BeginDelete { deep: true });
                }
                ui.separator();
                if ui.button("Close media").clicked() {
                    actions.push(Action::CloseMedia);
                }
                if ui.button("Forget password").clicked() {
                    actions.push(Action::ForgetPassword);
                }
                if ui.button("Clear decrypted cache").clicked() {
                    actions.push(Action::ClearCache);
                }
                if ui
                    .button("Lock all")
                    .on_hover_text("Close media, forget passwords, clear the cache")
                    .clicked()
                {
                    actions.push(Action::LockAll);
                }
            });
            if matches!(self.content, Content::Image(_) | Content::Pdf(_)) {
                ui.separator();
                if ui.button("−").on_hover_text("Zoom out").clicked() {
                    actions.push(Action::ZoomOut);
                }
                if ui.button("+").on_hover_text("Zoom in").clicked() {
                    actions.push(Action::ZoomIn);
                }
                if ui.button("100%").clicked() {
                    actions.push(Action::ActualSize);
                }
                if ui.button("Fit window").clicked() {
                    actions.push(Action::FitWindow);
                }
                if ui
                    .button("Fit image")
                    .on_hover_text("Fit, but never enlarge small images")
                    .clicked()
                {
                    actions.push(Action::FitImage);
                }
                if ui.button("Reset").clicked() {
                    actions.push(Action::ResetView);
                }
            }
            ui.separator();
            if ui
                .button("◀")
                .on_hover_text("Previous file (Left arrow)")
                .clicked()
            {
                actions.push(Action::Prev);
            }
            if ui
                .button("▶")
                .on_hover_text("Next file (Right arrow)")
                .clicked()
            {
                actions.push(Action::Next);
            }
            if ui
                .button("Theater")
                .on_hover_text("Borderless full screen, viewing pane only (F or F11)")
                .clicked()
            {
                actions.push(Action::ToggleTheater);
            }
            if ui.button("Help").clicked() {
                actions.push(Action::ToggleHelp);
            }
        });
    }

    fn ui_left(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, LeftTab::Files, "Files");
            ui.selectable_value(&mut self.tab, LeftTab::Favorites, "Favorites");
        });
        ui.separator();
        match self.tab {
            LeftTab::Files => self.ui_files(ui, actions),
            LeftTab::Favorites => self.ui_favorites(ui, actions),
        }
    }

    fn ui_files(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal_wrapped(|ui| {
            for root in roots() {
                if ui.small_button(root.display().to_string()).clicked() {
                    actions.push(Action::EnterDir(root));
                }
            }
            if let Some(home) = dirs::home_dir() {
                if ui.small_button("Home").clicked() {
                    actions.push(Action::EnterDir(home));
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Up").clicked() {
                actions.push(Action::GoUp);
            }
            let response = ui
                .add(egui::TextEdit::singleline(&mut self.path_input).desired_width(f32::INFINITY));
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                actions.push(Action::GoToInput);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.checkbox(&mut self.show_hidden, "Hidden").changed() {
                actions.push(Action::Refresh);
            }
            if ui.button("Refresh").clicked() {
                actions.push(Action::Refresh);
            }
            if ui
                .button("★ Favorite")
                .on_hover_text("Add this folder to favorites")
                .clicked()
            {
                actions.push(Action::AddFavorite);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Tick all files").clicked() {
                actions.push(Action::SelectAll);
            }
            if ui.small_button("Untick all").clicked() {
                actions.push(Action::ClearSelection);
            }
            ui.weak(format!("{} ticked", self.selection.len()));
        });
        if let Some(error) = &self.listing_error {
            ui.colored_label(BAD, error.as_str());
        }
        ui.separator();
        if self.entries.is_empty() {
            ui.weak("(empty folder)");
            return;
        }
        let row_height = ui.spacing().interact_size.y;
        let indent = ui.spacing().icon_width + ui.spacing().item_spacing.x;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, self.entries.len(), |ui, range| {
                for index in range {
                    let entry = &self.entries[index];
                    ui.horizontal(|ui| {
                        if entry.is_dir() {
                            ui.add_space(indent);
                        } else {
                            let mut checked = self.selection.contains(&entry.path);
                            if ui.checkbox(&mut checked, "").changed() {
                                let shift = ui.input(|i| i.modifiers.shift);
                                actions.push(Action::ToggleSelect { index, shift });
                            }
                        }
                        let (icon, color) = kind_style(entry.kind);
                        let is_current = self.nav.current() == Some(entry.path.as_path());
                        let label = RichText::new(format!("{icon} {}", entry.name)).color(color);
                        let hover = if entry.is_dir() {
                            "Open folder".to_string()
                        } else {
                            format!("{} bytes", entry.size)
                        };
                        if ui
                            .selectable_label(is_current, label)
                            .on_hover_text(hover)
                            .clicked()
                        {
                            if entry.is_dir() {
                                actions.push(Action::EnterDir(entry.path.clone()));
                            } else {
                                actions.push(Action::OpenFile(entry.path.clone()));
                            }
                        }
                    });
                }
            });
    }

    fn ui_favorites(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        if ui.small_button("Re-check").clicked() {
            actions.push(Action::RecheckFavorites);
        }
        if self.favorites.folders().is_empty() {
            ui.weak("No favorite folders yet. Open a folder under Files and press ★ Favorite.");
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (folder, status) in &self.fav_status {
                    ui.horizontal(|ui| {
                        let usable = *status == FavoriteStatus::Available;
                        let name = format!("★ {}", folder.display());
                        if ui.add_enabled(usable, egui::Button::new(name)).clicked() {
                            actions.push(Action::OpenFavorite(folder.clone()));
                        }
                        match status {
                            FavoriteStatus::Available => {}
                            FavoriteStatus::Missing => {
                                ui.colored_label(WARN, "missing");
                            }
                            FavoriteStatus::NotADirectory => {
                                ui.colored_label(WARN, "not a folder");
                            }
                            FavoriteStatus::Inaccessible => {
                                ui.colored_label(WARN, "inaccessible");
                            }
                        }
                        if ui.small_button("Remove").clicked() {
                            actions.push(Action::RemoveFavorite(folder.clone()));
                        }
                    });
                }
            });
    }

    #[cfg_attr(not(feature = "video"), allow(unused_variables, clippy::ptr_arg))]
    fn ui_viewer(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        match &mut self.content {
            Content::Empty => {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("Select an image, video, audio file or PDF in the file browser.\nLeft/Right arrows move between files.")
                            .color(Color32::GRAY),
                    );
                });
            }
            Content::Failed { message, .. } => {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(BAD, message.as_str());
                });
            }
            Content::Image(view) => {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                let viewport = (rect.width(), rect.height());
                view.viewport = viewport;
                let size = view.size;
                view_input(ui, &response, rect, &mut view.transform, size);
                if let Some((texture, _)) = view.textures.get(view.index) {
                    paint_texture(ui, rect, texture, &view.transform, size);
                }
            }
            Content::Pdf(view) => {
                let available = ui.available_size();
                let area = egui::vec2(
                    available.x,
                    (available.y - c::PDF_CONTROLS_HEIGHT).max(80.0),
                );
                let (rect, response) = ui.allocate_exact_size(area, egui::Sense::click_and_drag());
                view.viewport = (rect.width(), rect.height());
                let size = view.size_pt();
                view_input(ui, &response, rect, &mut view.transform, size);
                match (&view.shown, &view.page_error) {
                    (Some(shown), _) => {
                        paint_texture(ui, rect, &shown.texture, &view.transform, size)
                    }
                    (None, Some(message)) => centered_text(
                        ui,
                        rect,
                        format!("This page could not be displayed:\n{message}"),
                        BAD,
                    ),
                    (None, None) => centered_text(
                        ui,
                        rect,
                        if view.loading() {
                            "Loading PDF…"
                        } else {
                            "Rendering page…"
                        }
                        .to_string(),
                        Color32::GRAY,
                    ),
                }
                pdf_controls(ui, view, actions);
            }
            #[cfg(feature = "video")]
            Content::Video(view) => {
                let controls_height = 64.0;
                let available = ui.available_size();
                let area = egui::vec2(available.x, (available.y - controls_height).max(80.0));
                let (rect, response) = ui.allocate_exact_size(area, egui::Sense::click());
                if view.audio_view() {
                    draw_audio_view(ui, rect, view);
                } else {
                    match &view.texture {
                        Some(texture) => {
                            let size = (view.frame_size.0 as f32, view.frame_size.1 as f32);
                            paint_texture(ui, rect, texture, &ViewTransform::new(), size);
                        }
                        None => {
                            ui.painter_at(rect).text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "Loading video…",
                                egui::FontId::proportional(16.0),
                                Color32::GRAY,
                            );
                        }
                    }
                }
                if response.clicked() {
                    view.player.toggle();
                }
                if response.double_clicked() {
                    actions.push(Action::ToggleTheater);
                }
                if let Some(error) = view.player.error() {
                    ui.colored_label(BAD, format!("Playback error: {error}"));
                }
                video_controls(ui, view, actions);
            }
        }
    }

    fn status_text(&self) -> String {
        let position = self
            .nav
            .position()
            .map(|(n, total)| format!("  ({n}/{total})"))
            .unwrap_or_default();
        match &self.content {
            Content::Empty => "No file open".to_string(),
            Content::Failed { path, .. } => format!("{}  — could not be opened", file_label(path)),
            Content::Image(view) => {
                let mut text = file_label(&view.path);
                if view.encrypted {
                    text.push_str("  [encrypted, decrypted in memory]");
                }
                text.push_str(&format!("  {}×{}", view.original.0, view.original.1));
                if view.size.0 as u32 != view.original.0 {
                    text.push_str(" (downscaled for display)");
                }
                let ratio = view.size.0 / view.original.0.max(1) as f32;
                let scale = view.transform.scale(view.size, view.viewport) * ratio;
                text.push_str(&format!("  {:.0}%{position}", scale * 100.0));
                text
            }
            Content::Pdf(view) => {
                let mut text = file_label(&view.path);
                if view.encrypted {
                    text.push_str("  [encrypted, decrypted in memory]");
                }
                if view.loading() {
                    text.push_str("  loading…");
                } else {
                    let scale = view.transform.scale(view.size_pt(), view.viewport);
                    text.push_str(&format!(
                        "  page {}/{}  {:.0}%{position}",
                        view.page + 1,
                        view.page_count(),
                        scale * 100.0
                    ));
                }
                text
            }
            #[cfg(feature = "video")]
            Content::Video(view) => {
                let mut text = file_label(&view.path);
                if view.encrypted {
                    text.push_str("  [encrypted, streamed from memory]");
                }
                text.push_str(&position);
                text
            }
        }
    }

    // ----- dialogs -----

    fn show_dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut event = DialogEvent::None;
        let mut keep = true;
        let compress = &mut self.compress;
        match &mut dialog {
            Dialog::Unlock {
                path,
                password,
                error,
                focus,
            } => {
                egui::Window::new("Password required")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("Enter the password for {}", file_label(path)));
                        if let Some(message) = error.as_ref() {
                            ui.colored_label(BAD, message.as_str());
                        }
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut **password)
                                .password(true)
                                .hint_text("Password")
                                .desired_width(280.0),
                        );
                        if *focus {
                            response.request_focus();
                            *focus = false;
                        }
                        let submit =
                            response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        ui.horizontal(|ui| {
                            if (ui.button("Open").clicked() || submit) && !password.is_empty() {
                                let entered = std::mem::replace(
                                    &mut *password,
                                    Zeroizing::new(String::new()),
                                );
                                event = DialogEvent::Unlock(path.clone(), entered);
                                keep = false;
                            }
                            if ui.button("Cancel").clicked() {
                                keep = false;
                            }
                        });
                    });
            }
            Dialog::Encrypt(d) => {
                egui::Window::new("Encrypt files")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("{} file(s) will be encrypted in place with one password.", d.files.len()));
                        ui.weak("Each file gets its own random key; the password protects those keys. Every file stays independently decryptable.");
                        for path in d.files.iter().take(6) {
                            ui.label(format!("   {}", file_label(path)));
                        }
                        if d.files.len() > 6 {
                            ui.weak(format!("   … and {} more", d.files.len() - 6));
                        }
                        ui.add_space(6.0);
                        ui.label("Password");
                        ui.add(
                            egui::TextEdit::singleline(&mut *d.password)
                                .password(!d.reveal)
                                .desired_width(300.0),
                        );
                        ui.label("Repeat password");
                        ui.add(
                            egui::TextEdit::singleline(&mut *d.confirm)
                                .password(!d.reveal)
                                .desired_width(300.0),
                        );
                        ui.checkbox(&mut d.reveal, "Show password");
                        let policy = workflow::validate_password(&d.password).err();
                        let mismatch = *d.password != *d.confirm;
                        if !d.password.is_empty() {
                            if let Some(problem) = &policy {
                                ui.colored_label(WARN, problem.to_string());
                            } else if mismatch {
                                ui.colored_label(WARN, "The two passwords do not match.");
                            }
                        }
                        ui.add_space(4.0);
                        ui.colored_label(
                            WARN,
                            "If you lose this password the encrypted files cannot be recovered. There is no recovery mechanism.",
                        );
                        ui.checkbox(&mut d.acknowledged, "I understand that a lost password means permanent data loss");
                        ui.checkbox(compress, "Compress before encrypting when it helps");
                        let valid = policy.is_none() && !mismatch && d.acknowledged && !d.files.is_empty();
                        ui.horizontal(|ui| {
                            if ui.add_enabled(valid, egui::Button::new("Encrypt")).clicked() {
                                let files = std::mem::take(&mut d.files);
                                let secret = std::mem::replace(&mut d.password, Zeroizing::new(String::new()));
                                event = DialogEvent::StartEncrypt(files, secret);
                                keep = false;
                            }
                            if ui.button("Cancel").clicked() {
                                keep = false;
                            }
                        });
                    });
            }
            Dialog::Delete {
                files,
                deep,
                acknowledged,
            } => {
                let title = if *deep {
                    "Deep delete files"
                } else {
                    "Delete files"
                };
                egui::Window::new(title)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("{} file(s) selected:", files.len()));
                        for path in files.iter().take(8) {
                            ui.label(format!("   {}", file_label(path)));
                        }
                        if files.len() > 8 {
                            ui.weak(format!("   … and {} more", files.len() - 8));
                        }
                        ui.add_space(6.0);
                        if *deep {
                            ui.colored_label(
                                BAD,
                                format!(
                                    "Each file is overwritten {} times with random data and then deleted. This cannot be undone.",
                                    c::DEEP_DELETE_PASSES
                                ),
                            );
                            ui.label(
                                "Overwriting does not guarantee that the data is physically gone on SSDs, wear-levelled or \
                                 journaling/copy-on-write storage, snapshots or backups. Earlier plaintext copies of an \
                                 encrypted file are not affected.",
                            );
                            ui.checkbox(acknowledged, "I understand that physical erasure cannot be guaranteed");
                        } else {
                            ui.label("The files are deleted with the operating system's standard file deletion. This cannot be undone from here.");
                            *acknowledged = true;
                        }
                        ui.horizontal(|ui| {
                            let label = if *deep { "Deep delete" } else { "Delete" };
                            if ui.add_enabled(*acknowledged, egui::Button::new(label)).clicked() {
                                event = DialogEvent::StartDelete(std::mem::take(files), *deep);
                                keep = false;
                            }
                            if ui.button("Cancel").clicked() {
                                keep = false;
                            }
                        });
                    });
            }
            Dialog::Report(report) => {
                egui::Window::new(report.title.as_str())
                    .collapsible(false)
                    .default_width(560.0)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(report.summary.as_str());
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                for (severity, line) in &report.lines {
                                    ui.colored_label(severity_color(*severity), line.as_str());
                                }
                            });
                        ui.separator();
                        if ui.button("OK").clicked() {
                            keep = false;
                        }
                    });
            }
        }
        if keep {
            self.dialog = Some(dialog);
        }
        match event {
            DialogEvent::None => {}
            DialogEvent::Unlock(path, password) => {
                self.open_media(ctx, path, Some(password.as_str()))
            }
            DialogEvent::StartEncrypt(files, password) => self.spawn_encrypt(ctx, files, password),
            DialogEvent::StartDelete(files, deep) => self.spawn_delete(ctx, files, deep),
        }
    }

    fn show_job_window(&self, ctx: &egui::Context) {
        let Some(job) = &self.job else {
            return;
        };
        egui::Window::new(job.title.as_str())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let fraction = if job.total == 0 {
                    0.0
                } else {
                    job.index as f32 / job.total as f32
                };
                let shown = (job.index + 1).min(job.total.max(1));
                ui.add(
                    egui::ProgressBar::new(fraction).text(format!("file {shown} of {}", job.total)),
                );
                ui.label(job.current.as_str());
                ui.weak("Please wait; do not close the application until this finishes.");
            });
    }

    // ----- boss key and theater mode -----

    /// Starts entering or leaving full screen for theater mode. The steps (for
    /// example restoring a maximized window first) follow the configured
    /// strategy; see [`WindowSequencer`].
    fn set_theater_window(&mut self, ctx: &egui::Context, on: bool) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let cmds = if on {
            self.window.enter(maximized)
        } else {
            self.window.leave()
        };
        Self::send_window_cmds(ctx, &cmds);
    }

    fn send_window_cmds(ctx: &egui::Context, cmds: &[WindowCmd]) {
        for cmd in cmds {
            ctx.send_viewport_cmd(match *cmd {
                WindowCmd::Maximize(on) => egui::ViewportCommand::Maximized(on),
                WindowCmd::Fullscreen(on) => egui::ViewportCommand::Fullscreen(on),
                WindowCmd::Decorations(on) => egui::ViewportCommand::Decorations(on),
            });
        }
    }

    /// Esc is the boss key. It works in every state of the main window (also
    /// while a text box or dialog has focus) and runs the existing Lock All
    /// action, then discards open dialogs (they may hold typed passwords),
    /// leaves theater mode and, a few frames later, minimizes the window.
    fn handle_boss_key(&mut self, ctx: &egui::Context) {
        let pressed = ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
        if pressed && self.boss.begin(self.theater) {
            self.apply(ctx, Action::LockAll);
            self.dialog = None;
            self.show_help = false;
            if self.theater {
                self.theater = false;
                self.set_theater_window(ctx, false);
            }
            ctx.request_repaint();
        }
    }

    /// Minimizes the window once the cleared frame has been presented and
    /// full screen has ended (see [`BossSequence`]).
    fn advance_boss(&mut self, ctx: &egui::Context) {
        if !self.boss.is_active() {
            return;
        }
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        if self.boss.advance(fullscreen) == BossStep::Minimize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if self.boss.is_active() {
            ctx.request_repaint();
        }
    }

    fn update_hud(&mut self, ctx: &egui::Context) {
        if !self.theater {
            return;
        }
        let active = ctx.input(|i| {
            i.pointer.delta() != egui::Vec2::ZERO
                || i.pointer.any_pressed()
                || i.smooth_scroll_delta != egui::Vec2::ZERO
                || i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
        });
        if active {
            self.hud.poke(self.now);
        }
    }

    /// The auto-hiding overlay of theater mode: exit, file navigation and zoom.
    fn ui_theater_hud(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.hud.visible(self.now) {
            if self.dialog.is_none() && self.job.is_none() {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            }
            return;
        }
        ctx.request_repaint_after(Duration::from_secs_f64(self.hud.remaining(self.now) + 0.05));
        let zoomable = matches!(self.content, Content::Image(_) | Content::Pdf(_));
        let name = self
            .content_path()
            .map(|p| file_label(&p))
            .unwrap_or_default();
        egui::Area::new(egui::Id::new("theater-hud"))
            .anchor(egui::Align2::CENTER_TOP, [0.0, c::THEATER_HUD_MARGIN])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button("Exit theater")
                            .on_hover_text("Leave theater mode (F or F11)")
                            .clicked()
                        {
                            actions.push(Action::ToggleTheater);
                        }
                        ui.separator();
                        if ui
                            .button("◀")
                            .on_hover_text("Previous file (Left arrow)")
                            .clicked()
                        {
                            actions.push(Action::Prev);
                        }
                        if ui
                            .button("▶")
                            .on_hover_text("Next file (Right arrow)")
                            .clicked()
                        {
                            actions.push(Action::Next);
                        }
                        if zoomable {
                            ui.separator();
                            if ui.button("−").on_hover_text("Zoom out").clicked() {
                                actions.push(Action::ZoomOut);
                            }
                            if ui.button("+").on_hover_text("Zoom in").clicked() {
                                actions.push(Action::ZoomIn);
                            }
                            if ui.button("100%").clicked() {
                                actions.push(Action::ActualSize);
                            }
                            if ui.button("Fit window").clicked() {
                                actions.push(Action::FitWindow);
                            }
                        }
                        if !name.is_empty() {
                            ui.separator();
                            ui.label(name);
                        }
                        ui.separator();
                        ui.weak("Esc: lock and minimize");
                    });
                });
            });
    }

    fn show_help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_help;
        egui::Window::new("Help").open(&mut open).default_width(420.0).show(ctx, |ui| {
            ui.label("Left / Right: previous / next file     F or F11: theater mode (borderless full screen)");
            ui.label("Esc: boss key. Locks everything, leaves theater mode and minimizes the window");
            ui.label("+ / −: zoom     1: 100%     2: fit image     0: reset     W A S D: pan (or drag)");
            ui.label("Mouse wheel: zoom at the pointer     Double-click: toggle 100% / fit");
            ui.label("PDF: Page Up / Page Down / Home / End: change page");
            ui.label("Video and audio: Space or K: play/pause     J / L: back / forward     Up / Down: volume     M: mute");
            ui.separator();
            ui.label("Tick files with the check boxes (Shift extends a range), then use Encrypt, Delete or Deep delete.");
            ui.label("Encrypted files show a lock and are decrypted only in memory while viewed.");
            ui.label("Forget password, Clear decrypted cache and Close media are in the top bar; Lock all does all three.");
            ui.colored_label(WARN, "A lost password cannot be recovered. Deep delete cannot guarantee physical erasure.");
        });
        self.show_help = open;
    }
}

/// Mouse handling shared by images and PDF pages: wheel or pinch zooms at the
/// pointer, dragging pans, double-click toggles 100% and fit.
fn view_input(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    transform: &mut ViewTransform,
    size: (f32, f32),
) {
    let viewport = (rect.width(), rect.height());
    if response.hovered() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = pinch * (scroll / 300.0).exp();
        if (factor - 1.0).abs() > 1e-4 {
            if let Some(pointer) = response.hover_pos() {
                let anchor = (pointer.x - rect.center().x, pointer.y - rect.center().y);
                transform.zoom_at(factor, anchor, size, viewport);
            }
        }
    }
    if response.dragged() {
        let delta = response.drag_delta();
        transform.pan_by((delta.x, delta.y), size, viewport);
    }
    if response.double_clicked() {
        if transform.mode() == crate::viewer::ViewMode::Manual {
            transform.fit_window();
        } else {
            transform.actual_size();
        }
    }
}

/// Draws wrapped text centred in `rect`.
fn centered_text(ui: &egui::Ui, rect: egui::Rect, text: String, color: Color32) {
    let painter = ui.painter_at(rect);
    let galley = painter.layout(
        text,
        egui::FontId::proportional(16.0),
        color,
        (rect.width() - 48.0).max(50.0),
    );
    let pos = rect.center() - galley.size() / 2.0;
    painter.galley(pos, galley, color);
}

fn pdf_controls(ui: &mut egui::Ui, view: &PdfView, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let count = view.page_count().max(1);
        if ui.button("|◀").on_hover_text("First page (Home)").clicked() {
            actions.push(Action::Pdf(PageStep::First));
        }
        if ui
            .button("◀")
            .on_hover_text("Previous page (Page Up)")
            .clicked()
        {
            actions.push(Action::Pdf(PageStep::Prev));
        }
        let mut page = view.page + 1;
        if ui
            .add(egui::DragValue::new(&mut page).range(1..=count).speed(0.1))
            .changed()
        {
            actions.push(Action::Pdf(PageStep::Goto(page.saturating_sub(1))));
        }
        ui.label(format!("/ {count}"));
        if ui
            .button("▶")
            .on_hover_text("Next page (Page Down)")
            .clicked()
        {
            actions.push(Action::Pdf(PageStep::Next));
        }
        if ui.button("▶|").on_hover_text("Last page (End)").clicked() {
            actions.push(Action::Pdf(PageStep::Last));
        }
        if view.loading() {
            ui.weak("Loading…");
        }
        if view.degraded {
            ui.colored_label(WARN, "Some content on this page may not display correctly.");
        }
    });
}

#[cfg(feature = "video")]
fn draw_audio_view(ui: &egui::Ui, rect: egui::Rect, view: &VideoView) {
    let painter = ui.painter_at(rect);
    let margin = c::AUDIO_VIEW_MARGIN;
    let inner = rect.shrink(margin);
    let info = view.player.info();
    let fallback = view
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_label(&view.path));

    let mut text_area = inner;
    if let Some(cover) = &view.cover {
        let side = inner.height().min(inner.width() * 0.45).max(1.0);
        let slot = egui::Rect::from_min_size(
            egui::pos2(inner.left(), inner.center().y - side / 2.0),
            egui::vec2(side, side),
        );
        let texture_size = cover.size_vec2();
        let scale =
            (slot.width() / texture_size.x.max(1.0)).min(slot.height() / texture_size.y.max(1.0));
        let image_rect = egui::Rect::from_center_size(slot.center(), texture_size * scale);
        painter.image(
            cover.id(),
            image_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        text_area =
            egui::Rect::from_min_max(egui::pos2(slot.right() + margin, inner.top()), inner.max);
    }

    let mut lines: Vec<(String, f32, Color32)> =
        vec![(info.title_or(&fallback).to_string(), 26.0, Color32::WHITE)];
    if let Some(artist) = &info.artist {
        lines.push((artist.clone(), 18.0, Color32::LIGHT_GRAY));
    }
    if let Some(album) = &info.album {
        lines.push((album.clone(), 16.0, Color32::GRAY));
    }
    if let Some(genre) = &info.genre {
        lines.push((genre.clone(), 14.0, Color32::GRAY));
    }
    if let Some(details) = info.details_line() {
        lines.push((details, 14.0, Color32::GRAY));
    }
    let wrap = text_area.width().max(50.0);
    let galleys: Vec<_> = lines
        .into_iter()
        .map(|(text, size, color)| {
            painter.layout(text, egui::FontId::proportional(size), color, wrap)
        })
        .collect();
    let spacing = 8.0;
    let total: f32 = galleys.iter().map(|g| g.size().y).sum::<f32>()
        + spacing * galleys.len().saturating_sub(1) as f32;
    let mut y = text_area.center().y - total / 2.0;
    for galley in galleys {
        let height = galley.size().y;
        let x = text_area.center().x - galley.size().x / 2.0;
        painter.galley(egui::pos2(x, y), galley, Color32::WHITE);
        y += height + spacing;
    }
}

#[cfg(feature = "video")]
fn video_controls(ui: &mut egui::Ui, view: &mut VideoView, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let playing = view.player.is_playing();
        if ui.button(if playing { "Pause" } else { "Play" }).clicked() {
            view.player.toggle();
        }
        let position = view.player.position().unwrap_or(0.0);
        let duration = view.player.duration().unwrap_or(0.0).max(0.0);
        let upper = duration.max(0.001);
        let mut value = view.seek_preview.unwrap_or(position).clamp(0.0, upper);
        ui.spacing_mut().slider_width = (ui.available_width() - 460.0).max(120.0);
        let response = ui.add(egui::Slider::new(&mut value, 0.0..=upper).show_value(false));
        if response.changed() {
            view.seek_preview = Some(value);
        }
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            view.player.seek_to(value);
            view.seek_preview = None;
        }
        ui.label(format!(
            "{} / {}",
            fmt_time(view.seek_preview.unwrap_or(position)),
            fmt_time(duration)
        ));
        let mut volume = view.player.volume();
        ui.spacing_mut().slider_width = 80.0;
        if ui
            .add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false))
            .changed()
        {
            view.player.set_volume(volume);
        }
        let mut muted = view.player.is_muted();
        if ui.checkbox(&mut muted, "Mute").changed() {
            view.player.set_muted(muted);
        }
        let audio_count = view.player.audio_track_count();
        if audio_count > 1 {
            let current = view.player.audio_track().max(0);
            egui::ComboBox::from_id_salt("audio-track")
                .selected_text(format!("Audio {}", current + 1))
                .show_ui(ui, |ui| {
                    for index in 0..audio_count {
                        if ui
                            .selectable_label(index == current, format!("Track {}", index + 1))
                            .clicked()
                        {
                            view.player.set_audio_track(index);
                        }
                    }
                });
        }
        let text_count = view.player.subtitle_track_count();
        if text_count > 0 {
            let enabled = view.player.subtitles_enabled();
            let current = view.player.subtitle_track().max(0);
            let label = if enabled {
                format!("Subtitles {}", current + 1)
            } else {
                "Subtitles off".to_string()
            };
            egui::ComboBox::from_id_salt("subtitle-track")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(!enabled, "Off").clicked() {
                        view.player.set_subtitles_enabled(false);
                    }
                    for index in 0..text_count {
                        if ui
                            .selectable_label(
                                enabled && index == current,
                                format!("Track {}", index + 1),
                            )
                            .clicked()
                        {
                            view.player.set_subtitles_enabled(true);
                            view.player.set_subtitle_track(index);
                        }
                    }
                });
        }
        if ui
            .button("Theater")
            .on_hover_text("Borderless full screen (F or F11)")
            .clicked()
        {
            actions.push(Action::ToggleTheater);
        }
    });
}

impl eframe::App for VaultApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.now = ctx.input(|i| i.time);
        if matches!(&self.status, Some((_, expiry)) if self.now > *expiry) {
            self.status = None;
        }
        if self.status.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if self.tab == LeftTab::Favorites && self.now - self.fav_checked > 2.0 {
            self.refresh_favorite_status();
        }
        if self.tab == LeftTab::Favorites {
            ctx.request_repaint_after(Duration::from_secs(2));
        }

        self.handle_boss_key(ctx);
        self.update_hud(ctx);
        self.poll_job(ctx);
        self.tick_media(ctx);
        let keys = self.read_keys(ctx);
        self.apply_keys(ctx, keys);

        // Switch layouts only once the window itself has changed size, so the
        // new layout is never drawn at the old window size (that shows as flicker).
        let (reported_maximized, reported_fullscreen) = ctx.input(|i| {
            let viewport = i.viewport();
            (viewport.maximized, viewport.fullscreen)
        });
        let cmds = self.window.advance(reported_maximized, reported_fullscreen);
        Self::send_window_cmds(ctx, &cmds);
        if self.window.is_busy() {
            ctx.request_repaint();
        }
        let layout = self
            .layout
            .update(self.window.fullscreen_requested(), reported_fullscreen);
        if layout == LayoutState::Switching {
            ctx.request_repaint();
        }

        let mut actions: Vec<Action> = Vec::new();
        if layout == LayoutState::Normal {
            egui::TopBottomPanel::top("toolbar").show(ctx, |ui| self.ui_toolbar(ui, &mut actions));
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(self.status_text());
                    if let Some((message, _)) = &self.status {
                        ui.separator();
                        ui.label(message.as_str());
                    }
                });
            });
            egui::SidePanel::left("browser")
                .default_width(c::BROWSER_PANEL_WIDTH)
                .resizable(true)
                .show(ctx, |ui| self.ui_left(ui, &mut actions));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::from_gray(c::VIEWER_BACKGROUND_GRAY)))
            .show(ctx, |ui| {
                if layout != LayoutState::Switching {
                    self.ui_viewer(ui, &mut actions);
                }
            });
        if layout == LayoutState::Theater {
            self.ui_theater_hud(ctx, &mut actions);
        }

        self.show_dialogs(ctx);
        self.show_job_window(ctx);
        self.show_help_window(ctx);

        for action in actions {
            self.apply(ctx, action);
        }
        self.advance_boss(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.content = Content::Empty;
        self.session.shutdown();
    }
}

impl Drop for VaultApp {
    fn drop(&mut self) {
        self.content = Content::Empty;
        self.session.shutdown();
    }
}
