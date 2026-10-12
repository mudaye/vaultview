#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::path::{Path, PathBuf};

#[test]
fn encryption_appends_the_extension_and_keeps_the_original() {
    assert_eq!(encrypted_path(Path::new("dir/holiday.jpg")), PathBuf::from("dir/holiday.jpg.vvenc"));
    assert_eq!(encrypted_path(Path::new("dir/README")), PathBuf::from("dir/README.vvenc"));
    assert_eq!(encrypted_path(Path::new(".env")), PathBuf::from(".env.vvenc"));
    assert_eq!(encrypted_path(Path::new("archive.tar.gz")), PathBuf::from("archive.tar.gz.vvenc"));
}

#[test]
fn extension_detection_is_case_insensitive_and_exact() {
    assert!(has_encrypted_extension(Path::new("a.jpg.vvenc")));
    assert!(has_encrypted_extension(Path::new("A.JPG.VVENC")));
    assert!(has_encrypted_extension(Path::new("dir/a.vvenc")));
    assert!(!has_encrypted_extension(Path::new("a.jpg")));
    assert!(!has_encrypted_extension(Path::new(".vvenc")));
    assert!(!has_encrypted_extension(Path::new("a.vvenc.jpg")));
    assert!(!has_encrypted_extension(Path::new("a.vvencx")));
    assert!(!has_encrypted_extension(Path::new("vvenc")));
}

#[test]
fn original_path_strips_only_the_encrypted_extension() {
    assert_eq!(original_path(Path::new("dir/a.jpg.vvenc")), Some(PathBuf::from("dir/a.jpg")));
    assert_eq!(original_path(Path::new("a.VVENC")), Some(PathBuf::from("a")));
    assert_eq!(original_path(Path::new(".vvenc")), None);
    assert_eq!(original_path(Path::new("a.jpg")), None);
    assert_eq!(original_path(Path::new("a.vvencx")), None);
}

#[test]
fn encrypting_then_restoring_the_name_is_lossless() {
    for name in ["holiday.jpg", "my file (1).png", "фото.jpg", "noext", ".hidden", "a.b.c.d"] {
        let original = Path::new("some/dir").join(name);
        assert_eq!(original_path(&encrypted_path(&original)), Some(original.clone()), "{name}");
    }
}

#[test]
fn temp_path_is_a_hidden_sibling_that_is_not_mistaken_for_encrypted() {
    let temp = temp_path_for(Path::new("dir/a.jpg"));
    assert_eq!(temp.parent(), Some(Path::new("dir")));
    let name = temp.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(name, ".a.jpg.vvtmp");
    assert!(!has_encrypted_extension(&temp));
    assert_ne!(temp, encrypted_path(Path::new("dir/a.jpg")));
}

#[test]
fn unique_path_returns_unused_names_unchanged_and_numbers_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.jpg");
    assert_eq!(unique_path(&a), a);

    std::fs::write(&a, b"x").unwrap();
    assert_eq!(unique_path(&a), dir.path().join("a (1).jpg"));

    std::fs::write(dir.path().join("a (1).jpg"), b"x").unwrap();
    assert_eq!(unique_path(&a), dir.path().join("a (2).jpg"));

    let notes = dir.path().join("notes");
    std::fs::write(&notes, b"x").unwrap();
    assert_eq!(unique_path(&notes), dir.path().join("notes (1)"));
}

#[test]
fn destination_restores_the_original_name_when_free() {
    let dir = tempfile::tempdir().unwrap();
    let encrypted = dir.path().join("a.jpg.vvenc");
    std::fs::write(&encrypted, b"x").unwrap();
    assert_eq!(
        decryption_destination(&encrypted),
        Destination::Restored(dir.path().join("a.jpg"))
    );
}

#[test]
fn destination_never_silently_overwrites_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let encrypted = dir.path().join("a.jpg.vvenc");
    std::fs::write(&encrypted, b"x").unwrap();
    std::fs::write(dir.path().join("a.jpg"), b"precious").unwrap();
    assert_eq!(
        decryption_destination(&encrypted),
        Destination::NeedsChoice { suggested: dir.path().join("a (1).jpg") }
    );
}

#[test]
fn destination_asks_the_user_when_the_name_cannot_be_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let renamed = dir.path().join("blob");
    std::fs::write(&renamed, b"x").unwrap();
    match decryption_destination(&renamed) {
        Destination::NeedsChoice { suggested } => {
            assert_eq!(suggested.parent(), Some(dir.path()));
            assert_eq!(
                suggested.extension().unwrap().to_string_lossy(),
                crate::constants::DECRYPTED_FALLBACK_EXTENSION
            );
            assert!(!suggested.exists());
        }
        other => panic!("unexpected destination {other:?}"),
    }
}