#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::{MAX_TEXTURE_SIDE, PDF_DEFAULT_PAGE_SIZE, PDF_MAX_RENDER_PIXELS};
use std::time::{Duration, Instant};

fn add_object(out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &str) {
    offsets.push(out.len());
    out.extend_from_slice(body.as_bytes());
}

/// Builds a minimal, valid PDF with one page per entry of `sizes` (width,
/// height in points). The left half of every page is painted black.
fn make_pdf(sizes: &[(u32, u32)]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    add_object(
        &mut out,
        &mut offsets,
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
    );
    let kids: Vec<String> = (0..sizes.len())
        .map(|i| format!("{} 0 R", 3 + 2 * i))
        .collect();
    add_object(
        &mut out,
        &mut offsets,
        &format!(
            "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
            kids.join(" "),
            sizes.len()
        ),
    );
    for (i, (w, h)) in sizes.iter().enumerate() {
        let page = 3 + 2 * i;
        let contents = page + 1;
        add_object(
            &mut out,
            &mut offsets,
            &format!(
                "{page} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents {contents} 0 R /Resources << >> >>\nendobj\n"
            ),
        );
        let stream = format!("0 0 0 rg\n0 0 {} {} re\nf\n", w / 2, h);
        add_object(
            &mut out,
            &mut offsets,
            &format!(
                "{contents} 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n",
                stream.len()
            ),
        );
    }
    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", offsets.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn pixel(image: &ImageData, x: u32, y: u32) -> [u8; 4] {
    let i = (y as usize * image.width as usize + x as usize) * 4;
    [
        image.rgba[i],
        image.rgba[i + 1],
        image.rgba[i + 2],
        image.rgba[i + 3],
    ]
}

fn collect_until(worker: &mut PdfWorker, done: impl Fn(&[PdfEvent]) -> bool) -> Vec<PdfEvent> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut all = Vec::new();
    while Instant::now() < deadline {
        all.extend(worker.poll());
        if done(&all) {
            return all;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    all
}

fn has_opened(events: &[PdfEvent]) -> bool {
    events.iter().any(|e| matches!(e, PdfEvent::Opened { .. }))
}

fn has_rendered(events: &[PdfEvent], page: usize) -> bool {
    events
        .iter()
        .any(|e| matches!(e, PdfEvent::Rendered { page: p, .. } if *p == page))
}

// ---------- pure helpers ----------

#[test]
fn invalid_page_sizes_are_replaced_by_the_default() {
    assert_eq!(sanitize_page_size((612.0, 792.0)), (612.0, 792.0));
    for bad in [
        (0.0, 100.0),
        (100.0, 0.0),
        (-5.0, 100.0),
        (f32::NAN, 100.0),
        (100.0, f32::INFINITY),
    ] {
        assert_eq!(sanitize_page_size(bad), PDF_DEFAULT_PAGE_SIZE, "{bad:?}");
    }
}

#[test]
fn render_scale_follows_display_scale_and_pixel_density() {
    let page = (612.0, 792.0);
    assert_eq!(render_scale(1.0, 1.0, page), 1.0);
    assert_eq!(render_scale(2.0, 1.0, page), 2.0);
    assert_eq!(render_scale(1.0, 2.0, page), 2.0);
    assert_eq!(render_scale(0.5, 1.0, page), 0.5);
}

#[test]
fn small_zoom_changes_do_not_change_the_render_scale() {
    let page = (612.0, 792.0);
    assert_eq!(render_scale(1.0, 1.0, page), render_scale(1.05, 1.0, page));
    assert_ne!(render_scale(1.0, 1.0, page), render_scale(1.3, 1.0, page));
}

#[test]
fn render_scale_never_decreases_when_zooming_in() {
    let page = (612.0, 792.0);
    let mut previous = 0.0;
    let mut zoom = 0.01f32;
    while zoom < 100.0 {
        let scale = render_scale(zoom, 1.0, page);
        assert!(scale >= previous, "zoom {zoom}: {scale} < {previous}");
        previous = scale;
        zoom *= 1.07;
    }
}

#[test]
fn render_scale_respects_texture_and_pixel_limits() {
    let page = (612.0f32, 792.0f32);
    let scale = render_scale(1000.0, 1.0, page);
    assert!(page.0.max(page.1) * scale <= MAX_TEXTURE_SIDE as f32 + 0.5);
    assert!(
        f64::from(page.0 * scale) * f64::from(page.1 * scale)
            <= PDF_MAX_RENDER_PIXELS as f64 * 1.01
    );

    let huge = (14_400.0f32, 14_400.0f32);
    let small = render_scale(8.0, 2.0, huge);
    assert!(small > 0.0 && small.is_finite());
    assert!(
        f64::from(huge.0 * small) * f64::from(huge.1 * small)
            <= PDF_MAX_RENDER_PIXELS as f64 * 1.01
    );
}

#[test]
fn render_scale_is_always_finite_and_positive() {
    let page = (612.0, 792.0);
    for (display, density) in [
        (f32::NAN, 1.0),
        (1.0, f32::NAN),
        (f32::INFINITY, 1.0),
        (0.0, 1.0),
        (-3.0, 1.0),
        (1.0, 0.0),
        (1e-12, 1.0),
    ] {
        let scale = render_scale(display, density, page);
        assert!(
            scale.is_finite() && scale > 0.0,
            "{display} {density}: {scale}"
        );
    }
    let weird_page = render_scale(1.0, 1.0, (0.0, f32::NAN));
    assert!(weird_page.is_finite() && weird_page > 0.0);
}

// ---------- document core ----------

#[test]
fn a_valid_document_opens_with_its_page_sizes() {
    let core = PdfCore::open(make_pdf(&[(200, 100), (100, 300)])).unwrap();
    assert_eq!(core.page_count(), 2);
    assert_eq!(core.sizes(), &[(200.0, 100.0), (100.0, 300.0)]);
}

#[test]
fn a_page_renders_to_the_expected_size_and_content() {
    let core = PdfCore::open(make_pdf(&[(200, 100)])).unwrap();
    let page = core.render(0, 1.0).unwrap();
    assert!(
        (page.image.width as i32 - 200).abs() <= 1,
        "{}",
        page.image.width
    );
    assert!(
        (page.image.height as i32 - 100).abs() <= 1,
        "{}",
        page.image.height
    );
    assert_eq!(
        page.image.rgba.len(),
        page.image.width as usize * page.image.height as usize * 4
    );
    let left = pixel(&page.image, 10, 50);
    let right = pixel(&page.image, 190, 50);
    assert!(
        left[0] < 10 && left[1] < 10 && left[2] < 10 && left[3] == 255,
        "{left:?}"
    );
    assert!(
        right[0] > 245 && right[1] > 245 && right[2] > 245 && right[3] == 255,
        "{right:?}"
    );
    assert!(!page.degraded);
}

#[test]
fn scale_changes_the_rendered_size() {
    let core = PdfCore::open(make_pdf(&[(200, 100)])).unwrap();
    let double = core.render(0, 2.0).unwrap();
    assert!((double.image.width as i32 - 400).abs() <= 1);
    assert!((double.image.height as i32 - 200).abs() <= 1);
    let half = core.render(0, 0.5).unwrap();
    assert!((half.image.width as i32 - 100).abs() <= 1);
}

#[test]
fn rendering_a_missing_page_is_an_error() {
    let core = PdfCore::open(make_pdf(&[(200, 100)])).unwrap();
    assert!(core.render(1, 1.0).is_err());
    assert!(core.render(usize::MAX, 1.0).is_err());
}

#[test]
fn invalid_scales_fall_back_to_a_sane_default() {
    let core = PdfCore::open(make_pdf(&[(200, 100)])).unwrap();
    for scale in [f32::NAN, 0.0, -1.0, f32::INFINITY] {
        let page = core.render(0, scale).unwrap();
        assert!((page.image.width as i32 - 200).abs() <= 1, "scale {scale}");
    }
}

#[test]
fn damaged_documents_fail_with_a_message_instead_of_panicking() {
    assert!(PdfCore::open(Vec::new()).is_err());
    assert!(PdfCore::open(b"this is not a pdf".to_vec()).is_err());
    assert!(PdfCore::open(b"%PDF-1.4\n".to_vec()).is_err());
    let valid = make_pdf(&[(200, 100)]);
    assert!(PdfCore::open(valid[..valid.len() / 3].to_vec()).is_err());
    let message = PdfCore::open(Vec::new()).err().unwrap();
    assert!(!message.is_empty());
}

// ---------- worker ----------

#[test]
fn the_worker_opens_a_document_and_renders_requested_pages() {
    let mut worker = PdfWorker::open(make_pdf(&[(200, 100), (100, 300)]));
    let events = collect_until(&mut worker, has_opened);
    match events.iter().find(|e| matches!(e, PdfEvent::Opened { .. })) {
        Some(PdfEvent::Opened { sizes }) => {
            assert_eq!(sizes, &vec![(200.0, 100.0), (100.0, 300.0)]);
        }
        _ => panic!("no Opened event: {events:?}"),
    }
    worker.request(1, 1.0);
    let events = collect_until(&mut worker, |e| has_rendered(e, 1));
    match events
        .iter()
        .find(|e| matches!(e, PdfEvent::Rendered { page: 1, .. }))
    {
        Some(PdfEvent::Rendered { image, scale, .. }) => {
            assert_eq!(*scale, 1.0);
            assert!((image.width as i32 - 100).abs() <= 1);
            assert!((image.height as i32 - 300).abs() <= 1);
        }
        _ => panic!("page 1 was not rendered: {events:?}"),
    }
}

#[test]
fn the_worker_reports_unopenable_documents_once_and_stops() {
    let mut worker = PdfWorker::open(b"definitely not a pdf".to_vec());
    let events = collect_until(&mut worker, |e| {
        e.iter().any(|x| matches!(x, PdfEvent::OpenFailed(_)))
    });
    assert!(events
        .iter()
        .any(|e| matches!(e, PdfEvent::OpenFailed(m) if !m.is_empty())));
    std::thread::sleep(Duration::from_millis(200));
    let later = worker.poll();
    assert!(
        !later.iter().any(|e| matches!(e, PdfEvent::Crashed)),
        "an open failure must not also be reported as a crash: {later:?}"
    );
}

#[test]
fn a_failed_page_does_not_stop_the_worker() {
    let mut worker = PdfWorker::open(make_pdf(&[(200, 100)]));
    collect_until(&mut worker, has_opened);
    worker.request(99, 1.0);
    let events = collect_until(&mut worker, |e| {
        e.iter()
            .any(|x| matches!(x, PdfEvent::RenderFailed { page: 99, .. }))
    });
    assert!(events
        .iter()
        .any(|e| matches!(e, PdfEvent::RenderFailed { page: 99, scale, .. } if *scale == 1.0)));
    worker.request(0, 1.0);
    let events = collect_until(&mut worker, |e| has_rendered(e, 0));
    assert!(
        has_rendered(&events, 0),
        "the worker must keep working: {events:?}"
    );
}

#[test]
fn only_the_newest_queued_request_is_guaranteed_to_be_rendered() {
    let mut worker = PdfWorker::open(make_pdf(&[(200, 100), (100, 300)]));
    collect_until(&mut worker, has_opened);
    for _ in 0..5 {
        worker.request(0, 1.0);
    }
    worker.request(1, 2.0);
    let events = collect_until(&mut worker, |e| has_rendered(e, 1));
    assert!(has_rendered(&events, 1), "{events:?}");
}

#[test]
fn dropping_the_handle_does_not_block() {
    let mut worker = PdfWorker::open(make_pdf(&[(200, 100)]));
    collect_until(&mut worker, has_opened);
    worker.request(0, 1.0);
    let started = Instant::now();
    drop(worker);
    assert!(started.elapsed() < Duration::from_secs(5));
}

// ---------- page navigation ----------

#[test]
fn page_steps_stay_within_the_document() {
    assert_eq!(target_page(3, 10, PageStep::First), 0);
    assert_eq!(target_page(3, 10, PageStep::Last), 9);
    assert_eq!(target_page(3, 10, PageStep::Prev), 2);
    assert_eq!(target_page(3, 10, PageStep::Next), 4);
    assert_eq!(target_page(0, 10, PageStep::Prev), 0);
    assert_eq!(target_page(9, 10, PageStep::Next), 9);
    assert_eq!(target_page(3, 10, PageStep::Goto(7)), 7);
    assert_eq!(target_page(3, 10, PageStep::Goto(99)), 9);
    assert_eq!(target_page(3, 10, PageStep::Goto(usize::MAX)), 9);
}

#[test]
fn page_steps_handle_empty_and_single_page_documents() {
    for step in [
        PageStep::First,
        PageStep::Prev,
        PageStep::Next,
        PageStep::Last,
        PageStep::Goto(5),
    ] {
        assert_eq!(target_page(0, 0, step), 0, "{step:?} on an empty document");
        assert_eq!(target_page(0, 1, step), 0, "{step:?} on a single page");
    }
}

#[test]
fn page_steps_recover_from_a_stale_current_page() {
    assert_eq!(target_page(50, 3, PageStep::Prev), 2);
    assert_eq!(target_page(50, 3, PageStep::Next), 2);
    assert_eq!(target_page(usize::MAX, 3, PageStep::Next), 2);
}
