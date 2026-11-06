//! PDF loading and page rendering with the pure-Rust `hayro` rasterizer.
//!
//! All parsing and rendering happens on a dedicated worker thread, so a slow
//! page never freezes the interface and a panic caused by a malformed or
//! unusual file is contained: it is reported as an error for that page (or
//! for the whole document) instead of crashing the application.
//!
//! Documents are parsed from memory only; nothing is written to disk. The
//! parser keeps its own copy of the bytes, which this application cannot
//! zeroize. PDFs protected by their own password are not supported and are
//! reported as errors.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, RenderCache, RenderSettings};

use crate::constants::{
    MAX_TEXTURE_SIDE, PDF_DEFAULT_PAGE_SIZE, PDF_MAX_RENDER_PIXELS, PDF_MIN_RENDER_SCALE,
    PDF_RENDER_STEPS_PER_OCTAVE,
};
use crate::media::ImageData;

/// Replaces zero, negative and non-finite page sizes with the default size.
pub fn sanitize_page_size(size: (f32, f32)) -> (f32, f32) {
    let valid = |v: f32| v.is_finite() && v > 0.0;
    if valid(size.0) && valid(size.1) {
        size
    } else {
        PDF_DEFAULT_PAGE_SIZE
    }
}

/// Rounds a scale to one of a few discrete steps per doubling, so that small
/// zoom changes do not trigger a new render.
fn quantize_scale(scale: f32) -> f32 {
    let steps = PDF_RENDER_STEPS_PER_OCTAVE;
    2f32.powf((scale.log2() * steps).round() / steps)
}

/// Chooses the render scale (pixels per page point) for a page.
///
/// `display_scale` is how many screen points one page point occupies and
/// `pixels_per_point` is the display's pixel density. The result is
/// quantized, never exceeds [`MAX_TEXTURE_SIDE`] on either side or the pixel
/// budget [`PDF_MAX_RENDER_PIXELS`], and is always positive and finite. Very
/// large pages at very high zoom are therefore rendered at reduced resolution.
pub fn render_scale(display_scale: f32, pixels_per_point: f32, page_size: (f32, f32)) -> f32 {
    let (width, height) = sanitize_page_size(page_size);
    let wanted = display_scale * pixels_per_point;
    let wanted = if wanted.is_finite() && wanted > 0.0 {
        wanted
    } else {
        1.0
    };
    let quantized = quantize_scale(wanted.max(PDF_MIN_RENDER_SCALE));
    let side_limit = MAX_TEXTURE_SIDE as f32 / width.max(height);
    let pixel_limit = (PDF_MAX_RENDER_PIXELS as f32 / (width * height)).sqrt();
    quantized.min(side_limit).min(pixel_limit).max(0.001)
}

/// A page-navigation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStep {
    /// The first page.
    First,
    /// The page before the current one.
    Prev,
    /// The page after the current one.
    Next,
    /// The last page.
    Last,
    /// A specific zero-based page.
    Goto(usize),
}

/// The page to show after `step`; always within `0..count` (zero for an empty
/// document), whatever the inputs.
pub fn target_page(current: usize, count: usize, step: PageStep) -> usize {
    let last = count.saturating_sub(1);
    match step {
        PageStep::First => 0,
        PageStep::Prev => current.saturating_sub(1).min(last),
        PageStep::Next => current.saturating_add(1).min(last),
        PageStep::Last => last,
        PageStep::Goto(page) => page.min(last),
    }
}

/// A rendered page.
#[derive(Debug)]
pub struct RenderedPage {
    /// RGBA pixels on an opaque white background.
    pub image: ImageData,
    /// `true` when the interpreter reported unsupported content on this page.
    pub degraded: bool,
}

/// A parsed document, used inside the worker thread.
pub struct PdfCore {
    pdf: Pdf,
    interpreter: InterpreterSettings,
    warnings: Arc<AtomicBool>,
    sizes: Vec<(f32, f32)>,
}

impl PdfCore {
    /// Parses a document from memory.
    ///
    /// Fails with a readable message for damaged, encrypted or empty files.
    pub fn open(data: Vec<u8>) -> Result<Self, String> {
        let pdf = Pdf::new(data).map_err(|error| {
            format!("this PDF could not be opened (it may be damaged, password-protected or use unsupported features): {error:?}")
        })?;
        let sizes: Vec<(f32, f32)> = pdf
            .pages()
            .iter()
            .map(|page| sanitize_page_size(page.render_dimensions()))
            .collect();
        if sizes.is_empty() {
            return Err("this PDF contains no pages".to_string());
        }
        let warnings = Arc::new(AtomicBool::new(false));
        let sink = warnings.clone();
        let interpreter = InterpreterSettings {
            warning_sink: Arc::new(move |_| sink.store(true, Ordering::Relaxed)),
            ..InterpreterSettings::default()
        };
        Ok(Self {
            pdf,
            interpreter,
            warnings,
            sizes,
        })
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.sizes.len()
    }

    /// Page sizes in points, indexed by page.
    pub fn sizes(&self) -> &[(f32, f32)] {
        &self.sizes
    }

    /// Renders a page at `scale` pixels per page point.
    pub fn render(&self, index: usize, scale: f32) -> Result<RenderedPage, String> {
        let pages = self.pdf.pages();
        let page = pages
            .get(index)
            .ok_or_else(|| format!("page {} does not exist", index.saturating_add(1)))?;
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let settings = RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..RenderSettings::default()
        };
        self.warnings.store(false, Ordering::Relaxed);
        // hayro ties a render cache to the page it was created for, so it
        // cannot be kept next to the document; a fresh one is used per render.
        let cache = RenderCache::new();
        let pixmap = render(page, &cache, &self.interpreter, &settings);
        let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        let rgba = pixmap.data_as_u8_slice().to_vec();
        if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
            return Err("the page produced an empty or inconsistent image".to_string());
        }
        Ok(RenderedPage {
            image: ImageData {
                width,
                height,
                rgba,
            },
            degraded: self.warnings.load(Ordering::Relaxed),
        })
    }
}

/// Something the worker reports back to the interface.
#[derive(Debug)]
pub enum PdfEvent {
    /// The document was parsed; page sizes are in points.
    Opened {
        /// Size of every page in points.
        sizes: Vec<(f32, f32)>,
    },
    /// The document could not be opened; the worker has stopped.
    OpenFailed(String),
    /// A page was rendered.
    Rendered {
        /// Zero-based page number.
        page: usize,
        /// The scale it was rendered at.
        scale: f32,
        /// The pixels.
        image: ImageData,
        /// `true` if unsupported content was skipped.
        degraded: bool,
    },
    /// A page could not be rendered; other pages are unaffected.
    RenderFailed {
        /// Zero-based page number.
        page: usize,
        /// The scale that was requested.
        scale: f32,
        /// What went wrong.
        message: String,
    },
    /// The worker ended unexpectedly.
    Crashed,
}

struct Request {
    page: usize,
    scale: f32,
}

/// Handle to the background worker of one open document.
///
/// Dropping the handle stops the worker after its current render.
pub struct PdfWorker {
    requests: Sender<Request>,
    events: Receiver<PdfEvent>,
    finished: bool,
}

impl PdfWorker {
    /// Starts a worker that parses `data` and then renders requested pages.
    pub fn open(data: Vec<u8>) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("pdf-worker".to_string())
            .spawn({
                let event_tx = event_tx.clone();
                move || worker_main(data, request_rx, event_tx)
            });
        if spawned.is_err() {
            let _ = event_tx.send(PdfEvent::OpenFailed(
                "the PDF renderer thread could not be started".to_string(),
            ));
        }
        Self {
            requests: request_tx,
            events: event_rx,
            finished: false,
        }
    }

    /// Asks for a page to be rendered. Requests that are still queued when a
    /// newer one arrives are skipped.
    pub fn request(&self, page: usize, scale: f32) {
        let _ = self.requests.send(Request { page, scale });
    }

    /// Collects everything the worker has reported so far.
    pub fn poll(&mut self) -> Vec<PdfEvent> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => {
                    if matches!(event, PdfEvent::OpenFailed(_) | PdfEvent::Crashed) {
                        self.finished = true;
                    }
                    out.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.finished {
                        self.finished = true;
                        out.push(PdfEvent::Crashed);
                    }
                    break;
                }
            }
        }
        out
    }
}

fn worker_main(data: Vec<u8>, requests: Receiver<Request>, events: Sender<PdfEvent>) {
    let core = match catch_unwind(AssertUnwindSafe(|| PdfCore::open(data))) {
        Ok(Ok(core)) => core,
        Ok(Err(message)) => {
            let _ = events.send(PdfEvent::OpenFailed(message));
            return;
        }
        Err(_) => {
            let _ = events.send(PdfEvent::OpenFailed(
                "this PDF could not be read: it is damaged or uses features this viewer cannot handle"
                    .to_string(),
            ));
            return;
        }
    };
    let sizes = core.sizes().to_vec();
    if events.send(PdfEvent::Opened { sizes }).is_err() {
        return;
    }
    while let Ok(mut request) = requests.recv() {
        while let Ok(newer) = requests.try_recv() {
            request = newer;
        }
        let (page, scale) = (request.page, request.scale);
        let reply = match catch_unwind(AssertUnwindSafe(|| core.render(page, scale))) {
            Ok(Ok(rendered)) => PdfEvent::Rendered {
                page,
                scale,
                image: rendered.image,
                degraded: rendered.degraded,
            },
            Ok(Err(message)) => PdfEvent::RenderFailed {
                page,
                scale,
                message,
            },
            Err(_) => PdfEvent::RenderFailed {
                page,
                scale,
                message:
                    "this page could not be rendered: it is damaged or uses unsupported content"
                        .to_string(),
            },
        };
        if events.send(reply).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests;
