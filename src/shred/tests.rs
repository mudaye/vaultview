#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::*;
use crate::error::Error;
use std::fs;
use std::io::Cursor;

#[test]
fn normal_delete_removes_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, b"data").unwrap();
    normal_delete(&path).unwrap();
    assert!(!path.exists());
}

#[test]
fn normal_delete_reports_missing_files_and_refuses_directories() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(normal_delete(&dir.path().join("nope")), Err(Error::Io(_))));
    let sub = dir.path().join("folder");
    fs::create_dir(&sub).unwrap();
    assert!(matches!(normal_delete(&sub), Err(Error::NotAFile(_))));
    assert!(sub.exists());
}

#[test]
fn deep_delete_uses_the_configured_number_of_passes_and_removes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret.bin");
    fs::write(&path, vec![0x5Au8; 10_000]).unwrap();

    let report = deep_delete(&path).unwrap();

    assert_eq!(report.passes, DEEP_DELETE_PASSES);
    assert_eq!(report.bytes_per_pass, 10_000);
    assert!(!path.exists());
}

#[test]
fn every_pass_overwrites_the_whole_file_with_fresh_random_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.bin");
    let original = vec![0xAAu8; 2 * SHRED_BUFFER_LEN + 12_345];
    fs::write(&path, &original).unwrap();

    let mut seen = Vec::new();
    let mut previous = original.clone();
    let report = deep_delete_observed(&path, 3, &mut |pass, p| {
        let now = fs::read(p).unwrap();
        assert_eq!(now.len(), original.len(), "length must not change during overwriting");
        assert_ne!(now, previous, "pass {pass} must change the contents");
        assert!(now.iter().any(|b| *b != now[0]), "pass {pass} must not be constant");
        assert!(now.iter().zip(&original).filter(|(a, b)| a != b).count() > now.len() / 2);
        previous = now;
        seen.push(pass);
    })
    .unwrap();

    assert_eq!(seen, vec![1, 2, 3]);
    assert_eq!(report.passes, 3);
    assert_eq!(report.bytes_per_pass, original.len() as u64);
    assert!(!path.exists());
}

#[test]
fn empty_files_can_be_deep_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty");
    fs::write(&path, b"").unwrap();
    let report = deep_delete(&path).unwrap();
    assert_eq!(report.bytes_per_pass, 0);
    assert!(!path.exists());
}

#[test]
fn zero_passes_is_rejected_and_nothing_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keep.bin");
    fs::write(&path, b"keep me").unwrap();
    let result = deep_delete_observed(&path, 0, &mut |_, _| {});
    assert!(matches!(result, Err(Error::InvalidArgument(_))));
    assert_eq!(fs::read(&path).unwrap(), b"keep me");
}

#[test]
fn missing_files_and_directories_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(deep_delete(&dir.path().join("nope")), Err(Error::Io(_))));
    let sub = dir.path().join("folder");
    fs::create_dir(&sub).unwrap();
    assert!(matches!(deep_delete(&sub), Err(Error::NotAFile(_))));
    assert!(sub.exists());
}

#[cfg(unix)]
#[test]
fn symlinks_are_refused_so_the_target_is_not_shredded() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.bin");
    fs::write(&target, b"target contents").unwrap();
    let link = dir.path().join("link.bin");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    assert!(matches!(deep_delete(&link), Err(Error::NotAFile(_))));
    assert_eq!(fs::read(&target).unwrap(), b"target contents");
    assert!(link.exists());
}

#[cfg(unix)]
#[test]
fn overwrite_failure_is_reported_and_the_file_is_not_claimed_shredded() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ro.bin");
    fs::write(&path, b"read only content").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(&path).is_ok() {
        return; // running with privileges that ignore permission bits
    }

    assert!(matches!(deep_delete(&path), Err(Error::Io(_))));
    assert!(path.exists());
    assert_eq!(fs::read(&path).unwrap(), b"read only content");
}

#[test]
fn encrypted_files_can_be_deep_deleted_like_any_other_file() {
    use crate::crypto::{encrypt_stream, EncryptOptions};
    use crate::format::KdfParams;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.vvenc");
    let opts = EncryptOptions {
        kdf: KdfParams {
            memory_kib: KDF_MEMORY_KIB_MIN,
            iterations: KDF_ITERATIONS_MIN,
            parallelism: KDF_PARALLELISM_MIN,
        },
        compress: false,
    };
    let mut file = fs::File::create(&path).unwrap();
    encrypt_stream(&mut Cursor::new(vec![7u8; 5000]), &mut file, "fixture-password-xyz", &opts).unwrap();
    drop(file);
    assert!(crate::format::is_encrypted_file(&path));

    let report = deep_delete(&path).unwrap();
    assert_eq!(report.passes, DEEP_DELETE_PASSES);
    assert!(!path.exists());
}