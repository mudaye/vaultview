//! Zoom, pan and fit calculations for the image viewer (pure logic, no UI).
//!
//! Coordinates: the pan offset moves the image centre relative to the
//! viewport centre; zoom anchors are measured from the viewport centre.

use crate::constants::{ZOOM_MAX, ZOOM_MIN, ZOOM_STEP_FACTOR};

/// A (width, height) pair in screen points.
pub type Size = (f32, f32);

/// How the image is scaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Fit the whole image inside the window, scaling up or down.
    FitWindow,
    /// Fit inside the window but never enlarge beyond 100%.
    FitImage,
    /// Explicit zoom factor with panning.
    Manual,
}

/// Scale and pan state of the viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    mode: ViewMode,
    zoom: f32,
    pan: (f32, f32),
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self::new()
    }
}

fn usable(image: Size, viewport: Size) -> bool {
    [image.0, image.1, viewport.0, viewport.1]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
}

impl ViewTransform {
    /// A new view that fits the image to the window.
    pub fn new() -> Self {
        Self {
            mode: ViewMode::FitWindow,
            zoom: 1.0,
            pan: (0.0, 0.0),
        }
    }

    /// The current scaling mode.
    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    /// The current pan offset.
    pub fn pan(&self) -> (f32, f32) {
        self.pan
    }

    /// The effective scale factor for the given image and viewport sizes.
    pub fn scale(&self, image: Size, viewport: Size) -> f32 {
        if !usable(image, viewport) {
            return if self.mode == ViewMode::Manual {
                self.zoom
            } else {
                1.0
            };
        }
        let fit = (viewport.0 / image.0).min(viewport.1 / image.1);
        match self.mode {
            ViewMode::FitWindow => fit,
            ViewMode::FitImage => fit.min(1.0),
            ViewMode::Manual => self.zoom,
        }
    }

    /// Shows the image at 100%.
    pub fn actual_size(&mut self) {
        self.mode = ViewMode::Manual;
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
    }

    /// Fits the image to the window.
    pub fn fit_window(&mut self) {
        self.mode = ViewMode::FitWindow;
        self.pan = (0.0, 0.0);
    }

    /// Fits the image to the window without enlarging small images.
    pub fn fit_image(&mut self) {
        self.mode = ViewMode::FitImage;
        self.pan = (0.0, 0.0);
    }

    /// Resets zoom and pan (fit to window).
    pub fn reset(&mut self) {
        self.fit_window();
    }

    /// Zooms in one step around the viewport centre.
    pub fn zoom_in(&mut self, image: Size, viewport: Size) {
        self.zoom_at(ZOOM_STEP_FACTOR, (0.0, 0.0), image, viewport);
    }

    /// Zooms out one step around the viewport centre.
    pub fn zoom_out(&mut self, image: Size, viewport: Size) {
        self.zoom_at(1.0 / ZOOM_STEP_FACTOR, (0.0, 0.0), image, viewport);
    }

    /// Multiplies the scale by `factor`, keeping the image point under
    /// `anchor` (relative to the viewport centre) fixed. The result is
    /// clamped to the configured zoom limits.
    pub fn zoom_at(&mut self, factor: f32, anchor: (f32, f32), image: Size, viewport: Size) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let current = self.scale(image, viewport);
        if !current.is_finite() || current <= 0.0 {
            return;
        }
        let target = (current * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let ratio = target / current;
        let base = if self.mode == ViewMode::Manual {
            self.pan
        } else {
            (0.0, 0.0)
        };
        self.pan = (
            anchor.0 - (anchor.0 - base.0) * ratio,
            anchor.1 - (anchor.1 - base.1) * ratio,
        );
        self.zoom = target;
        self.mode = ViewMode::Manual;
        self.clamp_pan(image, viewport);
    }

    /// Moves the image by `delta`; ignored unless zoomed (manual mode).
    pub fn pan_by(&mut self, delta: (f32, f32), image: Size, viewport: Size) {
        if self.mode != ViewMode::Manual {
            return;
        }
        self.pan = (self.pan.0 + delta.0, self.pan.1 + delta.1);
        self.clamp_pan(image, viewport);
    }

    /// The image rectangle `(x, y, width, height)` inside the viewport.
    pub fn image_rect(&self, image: Size, viewport: Size) -> (f32, f32, f32, f32) {
        let scale = self.scale(image, viewport);
        let (w, h) = (image.0 * scale, image.1 * scale);
        (
            (viewport.0 - w) / 2.0 + self.pan.0,
            (viewport.1 - h) / 2.0 + self.pan.1,
            w,
            h,
        )
    }

    fn clamp_pan(&mut self, image: Size, viewport: Size) {
        if self.mode != ViewMode::Manual {
            self.pan = (0.0, 0.0);
            return;
        }
        if !usable(image, viewport) {
            return;
        }
        let max_x = ((image.0 * self.zoom - viewport.0) / 2.0).max(0.0);
        let max_y = ((image.1 * self.zoom - viewport.1) / 2.0).max(0.0);
        self.pan = (
            self.pan.0.clamp(-max_x, max_x),
            self.pan.1.clamp(-max_y, max_y),
        );
    }
}

#[cfg(test)]
mod tests;
