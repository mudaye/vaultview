#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::MAGIC;
use crate::error::Error;
use std::cmp::Ordering;
use std::fs;
use std::path::PathBuf;

fn png_bytes() -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend_from_slice(&[0u8; 32]);
    v
}

fn jpeg_bytes() -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00];
    v.extend_from_slice(&[0u8; 32]);
    v
}

fn gif_bytes() -> Vec<u8> {
    let mut v = b"GIF89a".to_vec();
    v.extend_from_slice(&[0u8; 32]);
    v
}

fn avi_bytes() -> Vec<u8> {
    let mut v = b"RIFF\0\0\0\0AVI LIST".to_vec();
    v.extend_from_slice(&[0u8; 32]);
    v
}

fn zip_bytes() -> Vec<u8> {
    let mut v = vec![0x50, 0x4B, 0x03, 0x04];
    v.extend_from_slice(&[0u8; 32]);
    v
}

fn encrypted_bytes() -> Vec<u8> {
    let mut v = MAGIC.to_vec();
    v.extend_from_slice(&[0u8; 64]);
    v
}

fn entry(name: &str, kind: EntryKind) -> Entry {
    Entry { path: PathBuf::from("/media").join(name), name: name.to_string(), kind, size: 0 }
}

// ---------- classification ----------

#[test]
fn content_identifies_common_formats() {
    assert_eq!(classify_bytes(&png_bytes(), None), EntryKind::Image);
    assert_eq!(classify_bytes(&jpeg_bytes(), None), EntryKind::Image);
    assert_eq!(classify_bytes(&gif_bytes(), None), EntryKind::Image);
    assert_eq!(classify_bytes(&avi_bytes(), None), EntryKind::Video);
}

#[test]
fn content_wins_over_a_misleading_extension() {
    assert_eq!(classify_bytes(&png_bytes(), Some("mp4")), EntryKind::Image);
    assert_eq!(classify_bytes(&avi_bytes(), Some("jpg")), EntryKind::Video);
    assert_eq!(classify_bytes(&zip_bytes(), Some("jpg")), EntryKind::Other);
}

#[test]
fn extension_is_only_a_fallback_for_unrecognised_content() {
    let unknown = [0u8; 16];
    assert_eq!(classify_bytes(&unknown, Some("jpg")), EntryKind::Image);
    assert_eq!(classify_bytes(&unknown, Some("JPG")), EntryKind::Image);
    assert_eq!(classify_bytes(&unknown, Some("mp4")), EntryKind::Video);
    assert_eq!(classify_bytes(&unknown, Some("mkv")), EntryKind::Video);
    assert_eq!(classify_bytes(&unknown, Some("txt")), EntryKind::Other);
    assert_eq!(classify_bytes(&unknown, None), EntryKind::Other);
    assert_eq!(classify_bytes(b"hello world, plain text", None), EntryKind::Other);
}

#[test]
fn encrypted_files_are_recognised_by_magic_not_by_extension() {
    assert_eq!(classify_bytes(&encrypted_bytes(), Some("jpg")), EntryKind::Encrypted);
    assert_eq!(classify_bytes(&encrypted_bytes(), None), EntryKind::Encrypted);
    assert_eq!(classify_bytes(b"hello", Some("vvenc")), EntryKind::Other);
}

#[test]
fn classify_file_reads_the_file_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("picture.dat");
    fs::write(&png, png_bytes()).unwrap();
    assert_eq!(classify_file(&png).unwrap(), EntryKind::Image);

    let empty = dir.path().join("empty.png");
    fs::write(&empty, b"").unwrap();
    assert_eq!(classify_file(&empty).unwrap(), EntryKind::Image);

    assert!(matches!(classify_file(&dir.path().join("missing")), Err(Error::Io(_))));
}

// ---------- ordering ----------

#[test]
fn natural_ordering_is_case_insensitive_and_numeric_aware() {
    assert_eq!(natural_cmp("img2", "img10"), Ordering::Less);
    assert_eq!(natural_cmp("img10", "img2"), Ordering::Greater);
    assert_eq!(natural_cmp("10", "9"), Ordering::Greater);
    assert_eq!(natural_cmp("a", "B"), Ordering::Less);
    assert_eq!(natural_cmp("B", "a"), Ordering::Greater);
    assert_eq!(natural_cmp("a", "a1"), Ordering::Less);
    assert_eq!(natural_cmp("same", "same"), Ordering::Equal);
    assert_eq!(natural_cmp("file01", "file1"), Ordering::Less);
    assert_eq!(
        natural_cmp("x99999999999999999999999", "x100000000000000000000000"),
        Ordering::Less
    );
}

#[test]
fn natural_ordering_is_a_consistent_total_order() {
    let names = ["a", "A", "a1", "a01", "a10", "a2", "b", "B2", "img2", "img10"];
    for x in names {
        for y in names {
            assert_eq!(natural_cmp(x, y), natural_cmp(y, x).reverse(), "{x} vs {y}");
        }
    }
}

// ---------- directory listing ----------

fn populated_directory() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("b_dir")).unwrap();
    fs::create_dir(dir.path().join("A_dir")).unwrap();
    fs::write(dir.path().join("img10.png"), png_bytes()).unwrap();
    fs::write(dir.path().join("img2.png"), png_bytes()).unwrap();
    fs::write(dir.path().join("notes.txt"), b"plain notes").unwrap();
    fs::write(dir.path().join(".hidden.png"), png_bytes()).unwrap();
    fs::write(dir.path().join("secret.jpg.vvenc"), encrypted_bytes()).unwrap();
    dir
}

#[test]
fn listing_puts_directories_first_and_sorts_naturally() {
    let dir = populated_directory();
    let entries = list_directory(dir.path(), false).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["A_dir", "b_dir", "img2.png", "img10.png", "notes.txt", "secret.jpg.vvenc"]);

    let kinds: Vec<EntryKind> = entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EntryKind::Directory,
            EntryKind::Directory,
            EntryKind::Image,
            EntryKind::Image,
            EntryKind::Other,
            EntryKind::Encrypted
        ]
    );
    assert!(entries[0].is_dir());
    assert!(!entries[2].is_dir());
    assert_eq!(entries[2].path, dir.path().join("img2.png"));
    assert_eq!(entries[2].size, png_bytes().len() as u64);
}

#[test]
fn hidden_files_are_listed_only_on_request() {
    let dir = populated_directory();
    let hidden_shown = list_directory(dir.path(), true).unwrap();
    assert!(hidden_shown.iter().any(|e| e.name == ".hidden.png"));
    let hidden_not_shown = list_directory(dir.path(), false).unwrap();
    assert!(!hidden_not_shown.iter().any(|e| e.name == ".hidden.png"));
}

#[test]
fn listing_unreadable_locations_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(list_directory(&dir.path().join("nope"), false), Err(Error::Io(_))));
    let file = dir.path().join("file.txt");
    fs::write(&file, b"x").unwrap();
    assert!(list_directory(&file, false).is_err());
}

// ---------- navigation ----------

fn nav_entries() -> Vec<Entry> {
    vec![
        entry("folder", EntryKind::Directory),
        entry("img1.png", EntryKind::Image),
        entry("notes.txt", EntryKind::Other),
        entry("clip.mp4", EntryKind::Video),
        entry("secret.jpg.vvenc", EntryKind::Encrypted),
        entry("img2.png", EntryKind::Image),
    ]
}

#[test]
fn navigation_includes_images_videos_and_encrypted_files_only() {
    let entries = nav_entries();
    let nav = NavContext::from_entries(&entries);
    assert_eq!(nav.len(), 4);
    assert_eq!(nav.current(), None);
    assert_eq!(nav.position(), None);
}

#[test]
fn next_and_previous_skip_ineligible_entries() {
    let entries = nav_entries();
    let mut nav = NavContext::from_entries(&entries);
    assert!(nav.set_current(&entries[1].path));
    assert_eq!(nav.current(), Some(entries[1].path.as_path()));
    assert_eq!(nav.position(), Some((1, 4)));

    assert_eq!(nav.next_path(), Some(entries[3].path.as_path()));
    assert!(nav.set_current(&entries[3].path));
    assert_eq!(nav.next_path(), Some(entries[4].path.as_path()));
    assert_eq!(nav.previous_path(), Some(entries[1].path.as_path()));
    assert_eq!(nav.position(), Some((2, 4)));
}

#[test]
fn peeking_never_moves_the_cursor_so_a_failed_open_cannot_advance() {
    let entries = nav_entries();
    let mut nav = NavContext::from_entries(&entries);
    nav.set_current(&entries[4].path);
    let first = nav.next_path().map(|p| p.to_path_buf());
    let second = nav.next_path().map(|p| p.to_path_buf());
    assert_eq!(first, second);
    assert_eq!(nav.current(), Some(entries[4].path.as_path()));
}

#[test]
fn navigation_stops_at_both_ends() {
    let entries = nav_entries();
    let mut nav = NavContext::from_entries(&entries);
    nav.set_current(&entries[1].path);
    assert_eq!(nav.previous_path(), None);
    nav.set_current(&entries[5].path);
    assert_eq!(nav.next_path(), None);
    assert_eq!(nav.position(), Some((4, 4)));
}

#[test]
fn ineligible_or_unknown_paths_cannot_become_current() {
    let entries = nav_entries();
    let mut nav = NavContext::from_entries(&entries);
    nav.set_current(&entries[1].path);
    assert!(!nav.set_current(&entries[2].path));
    assert!(!nav.set_current(&entries[0].path));
    assert!(!nav.set_current(&PathBuf::from("/elsewhere/img.png")));
    assert_eq!(nav.current(), Some(entries[1].path.as_path()));
}

#[test]
fn navigation_without_a_current_file_or_without_media_yields_nothing() {
    let entries = nav_entries();
    let nav = NavContext::from_entries(&entries);
    assert_eq!(nav.next_path(), None);
    assert_eq!(nav.previous_path(), None);

    let empty = NavContext::from_entries(&[entry("notes.txt", EntryKind::Other)]);
    assert_eq!(empty.len(), 0);
    assert_eq!(empty.next_path(), None);
}

// ---------- selection ----------

#[test]
fn toggle_selects_and_deselects_files_but_never_directories() {
    let entries = nav_entries();
    let mut sel = Selection::default();
    assert!(sel.is_empty());

    assert!(!sel.toggle(&entries[0]));
    assert!(sel.is_empty());

    assert!(sel.toggle(&entries[1]));
    assert!(sel.contains(&entries[1].path));
    assert_eq!(sel.len(), 1);

    assert!(sel.toggle(&entries[1]));
    assert!(!sel.contains(&entries[1].path));
    assert!(sel.is_empty());
}

#[test]
fn range_selection_works_in_both_directions_and_skips_directories() {
    let entries = nav_entries();
    let mut sel = Selection::default();
    sel.select_range(&entries, 0, 2);
    assert!(sel.contains(&entries[1].path));
    assert!(sel.contains(&entries[2].path));
    assert!(!sel.contains(&entries[0].path));
    assert_eq!(sel.len(), 2);

    sel.select_range(&entries, 5, 4);
    assert!(sel.contains(&entries[4].path));
    assert!(sel.contains(&entries[5].path));
    assert_eq!(sel.len(), 4);

    sel.select_range(&entries, 3, 99);
    assert!(sel.contains(&entries[3].path));
    sel.select_range(&[], 0, 3);
}

#[test]
fn select_all_and_clear() {
    let entries = nav_entries();
    let mut sel = Selection::default();
    sel.select_all_files(&entries);
    assert_eq!(sel.len(), 5);
    assert!(!sel.contains(&entries[0].path));
    sel.clear();
    assert!(sel.is_empty());
}

#[test]
fn selected_paths_are_returned_in_sorted_order() {
    let entries = nav_entries();
    let mut sel = Selection::default();
    sel.toggle(&entries[5]);
    sel.toggle(&entries[1]);
    let paths = sel.paths();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
    assert_eq!(paths.len(), 2);
}

#[test]
fn only_unencrypted_files_are_offered_for_encryption_in_listing_order() {
    let entries = nav_entries();
    let mut sel = Selection::default();
    sel.select_all_files(&entries);
    let paths = encryptable_selection(&sel, &entries);
    assert_eq!(
        paths,
        vec![
            entries[1].path.clone(),
            entries[2].path.clone(),
            entries[3].path.clone(),
            entries[5].path.clone()
        ]
    );
}