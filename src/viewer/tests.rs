#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

const BIG: Size = (4000.0, 3000.0);
const VIEWPORT: Size = (1000.0, 1000.0);

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn a_new_view_fits_the_window_without_panning() {
    let t = ViewTransform::new();
    assert_eq!(t.mode(), ViewMode::FitWindow);
    assert_eq!(t.pan(), (0.0, 0.0));
    assert_eq!(ViewTransform::default(), t);
}

#[test]
fn fit_window_scales_up_and_down_but_fit_image_never_upscales() {
    let mut t = ViewTransform::new();
    assert!(approx(t.scale((2000.0, 1000.0), VIEWPORT), 0.5));
    assert!(approx(t.scale((100.0, 50.0), VIEWPORT), 10.0));

    t.fit_image();
    assert_eq!(t.mode(), ViewMode::FitImage);
    assert!(approx(t.scale((2000.0, 1000.0), VIEWPORT), 0.5));
    assert!(approx(t.scale((100.0, 50.0), VIEWPORT), 1.0));
}

#[test]
fn degenerate_sizes_never_produce_nan_or_infinity() {
    let t = ViewTransform::new();
    assert!(approx(t.scale((0.0, 0.0), VIEWPORT), 1.0));
    assert!(approx(t.scale(BIG, (0.0, 0.0)), 1.0));
    assert!(t.scale((f32::NAN, 10.0), VIEWPORT).is_finite());
}

#[test]
fn actual_size_is_one_hundred_percent() {
    let mut t = ViewTransform::new();
    t.actual_size();
    assert_eq!(t.mode(), ViewMode::Manual);
    assert!(approx(t.scale(BIG, VIEWPORT), 1.0));
    assert_eq!(t.pan(), (0.0, 0.0));
}

#[test]
fn zooming_starts_from_the_current_effective_scale() {
    let mut t = ViewTransform::new();
    let image = (2000.0, 1000.0);
    t.zoom_in(image, VIEWPORT);
    assert_eq!(t.mode(), ViewMode::Manual);
    assert!(approx(t.scale(image, VIEWPORT), 0.5 * ZOOM_STEP_FACTOR));
    t.zoom_out(image, VIEWPORT);
    assert!(approx(t.scale(image, VIEWPORT), 0.5));
}

#[test]
fn zoom_is_clamped_to_the_configured_limits() {
    let mut t = ViewTransform::new();
    t.actual_size();
    for _ in 0..200 {
        t.zoom_in(BIG, VIEWPORT);
    }
    assert!(approx(t.scale(BIG, VIEWPORT), ZOOM_MAX));
    for _ in 0..400 {
        t.zoom_out(BIG, VIEWPORT);
    }
    assert!(approx(t.scale(BIG, VIEWPORT), ZOOM_MIN));
}

#[test]
fn panning_is_clamped_to_the_scaled_image_edges() {
    let mut t = ViewTransform::new();
    t.actual_size();
    t.pan_by((10_000.0, 0.0), BIG, VIEWPORT);
    assert!(approx(t.pan().0, 1500.0));
    t.pan_by((0.0, -10_000.0), BIG, VIEWPORT);
    assert!(approx(t.pan().1, -1000.0));
    t.pan_by((-100.0, 0.0), BIG, VIEWPORT);
    assert!(approx(t.pan().0, 1400.0));
}

#[test]
fn images_smaller_than_the_viewport_cannot_be_panned() {
    let small: Size = (400.0, 300.0);
    let mut t = ViewTransform::new();
    t.actual_size();
    t.pan_by((50.0, 50.0), small, VIEWPORT);
    assert_eq!(t.pan(), (0.0, 0.0));
}

#[test]
fn panning_has_no_effect_while_the_whole_image_is_fitted() {
    let mut t = ViewTransform::new();
    t.pan_by((100.0, 100.0), BIG, VIEWPORT);
    assert_eq!(t.pan(), (0.0, 0.0));
    t.fit_image();
    t.pan_by((100.0, 100.0), BIG, VIEWPORT);
    assert_eq!(t.pan(), (0.0, 0.0));
}

#[test]
fn zooming_out_re_clamps_the_pan() {
    let mut t = ViewTransform::new();
    t.actual_size();
    t.pan_by((1500.0, 0.0), BIG, VIEWPORT);
    t.zoom_out(BIG, VIEWPORT);
    assert!(approx(t.scale(BIG, VIEWPORT), 1.0 / ZOOM_STEP_FACTOR));
    assert!(approx(t.pan().0, (4000.0 / ZOOM_STEP_FACTOR - 1000.0) / 2.0));
}

#[test]
fn zooming_at_a_point_keeps_that_image_point_under_the_cursor() {
    let mut t = ViewTransform::new();
    t.actual_size();
    t.zoom_at(2.0, (100.0, 0.0), BIG, VIEWPORT);
    assert!(approx(t.scale(BIG, VIEWPORT), 2.0));
    assert!(approx(t.pan().0, -100.0));
    assert!(approx(t.pan().1, 0.0));
}

#[test]
fn zooming_at_a_point_from_a_fit_mode_switches_to_manual() {
    let mut t = ViewTransform::new();
    let image = (2000.0, 1000.0);
    t.zoom_at(2.0, (0.0, 0.0), image, VIEWPORT);
    assert_eq!(t.mode(), ViewMode::Manual);
    assert!(approx(t.scale(image, VIEWPORT), 1.0));
    assert_eq!(t.pan(), (0.0, 0.0));
}

#[test]
fn zoom_at_respects_the_limits() {
    let mut t = ViewTransform::new();
    t.actual_size();
    t.zoom_at(1.0e9, (0.0, 0.0), BIG, VIEWPORT);
    assert!(approx(t.scale(BIG, VIEWPORT), ZOOM_MAX));
    t.zoom_at(1.0e-9, (0.0, 0.0), BIG, VIEWPORT);
    assert!(approx(t.scale(BIG, VIEWPORT), ZOOM_MIN));
}

#[test]
fn reset_and_fit_commands_clear_the_pan() {
    let mut t = ViewTransform::new();
    t.actual_size();
    t.pan_by((300.0, 200.0), BIG, VIEWPORT);
    assert_ne!(t.pan(), (0.0, 0.0));

    t.reset();
    assert_eq!(t.mode(), ViewMode::FitWindow);
    assert_eq!(t.pan(), (0.0, 0.0));

    t.actual_size();
    t.pan_by((300.0, 200.0), BIG, VIEWPORT);
    t.fit_window();
    assert_eq!(t.pan(), (0.0, 0.0));
    t.actual_size();
    t.pan_by((300.0, 200.0), BIG, VIEWPORT);
    t.fit_image();
    assert_eq!(t.pan(), (0.0, 0.0));
}

#[test]
fn image_rect_is_centred_and_follows_the_pan() {
    let small: Size = (400.0, 300.0);
    let mut t = ViewTransform::new();
    t.actual_size();
    let (x, y, w, h) = t.image_rect(small, VIEWPORT);
    assert!(approx(x, 300.0) && approx(y, 350.0) && approx(w, 400.0) && approx(h, 300.0));

    t.pan_by((1500.0, 0.0), BIG, VIEWPORT);
    let (x, _, w, _) = t.image_rect(BIG, VIEWPORT);
    assert!(approx(w, 4000.0));
    assert!(approx(x, 0.0), "left edge must align with the viewport after panning fully right");
}