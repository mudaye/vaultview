#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::*;
use crate::crypto::{decrypt_stream, EncryptOptions};
use crate::error::Error;
use crate::format::{is_encrypted_file, read_header, KdfParams};
use std::fs;
use std::path::{Path, PathBuf};

const PW: &str = "fixture-batch-password";

fn opts(compress: bool) -> EncryptOptions {
    EncryptOptions {
        kdf: KdfParams {
            memory_kib: KDF_MEMORY_KIB_MIN,
            iterations: KDF_ITERATIONS_MIN,
            parallelism: KDF_PARALLELISM_MIN,
        },
        compress,
    }
}

fn pattern(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x9E37_79B9;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        })
        .collect()
}

fn write_file(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, data).unwrap();
    path
}

fn decrypt_file(path: &Path, password: &str) -> Vec<u8> {
    let mut out = Vec::new();
    decrypt_stream(fs::File::open(path).unwrap(), &mut out, password).unwrap();
    out
}

fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn expect_encrypted(outcome: Outcome) -> PathBuf {
    match outcome {
        Outcome::Encrypted { output } => output,
        other => panic!("expected success, got {other:?}"),
    }
}

fn expect_failed(outcome: Outcome) -> Error {
    match outcome {
        Outcome::Failed { error } => error,
        other => panic!("expected failure, got {other:?}"),
    }
}

// ---------- password policy ----------

#[test]
fn password_policy_counts_characters_not_bytes() {
    assert!(matches!(
        validate_password(""),
        Err(Error::PasswordTooShort {
            min: MIN_PASSWORD_CHARS
        })
    ));
    assert!(matches!(
        validate_password("1234567"),
        Err(Error::PasswordTooShort { .. })
    ));
    assert!(validate_password("12345678").is_ok());
    assert!(matches!(
        validate_password("ééééééé"),
        Err(Error::PasswordTooShort { .. })
    ));
    assert!(validate_password("éééééééé").is_ok());
}

// ---------- single-file encryption ----------

#[test]
fn successful_encryption_replaces_the_file_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let data = pattern(150_000);
    let src = write_file(dir.path(), "a.jpg", &data);

    let output = expect_encrypted(encrypt_one(&src, PW, &opts(false)));

    assert_eq!(output, dir.path().join("a.jpg.vvenc"));
    assert!(!src.exists());
    assert_eq!(dir_names(dir.path()), vec!["a.jpg.vvenc"]);
    assert!(is_encrypted_file(&output));
    assert_eq!(decrypt_file(&output, PW), data);
}

#[test]
fn empty_and_compressible_files_encrypt_correctly() {
    let dir = tempfile::tempdir().unwrap();

    let empty = write_file(dir.path(), "empty.bin", b"");
    let empty_out = expect_encrypted(encrypt_one(&empty, PW, &opts(true)));
    assert!(decrypt_file(&empty_out, PW).is_empty());

    let text = b"the same line over and over\n".repeat(20_000);
    let src = write_file(dir.path(), "log.txt", &text);
    let out = expect_encrypted(encrypt_one(&src, PW, &opts(true)));
    assert!(fs::metadata(&out).unwrap().len() < (text.len() / 4) as u64);
    assert_eq!(decrypt_file(&out, PW), text);
}

#[test]
fn missing_files_and_directories_fail_without_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.png");
    assert!(matches!(
        expect_failed(encrypt_one(&missing, PW, &opts(false))),
        Error::Io(_)
    ));

    let sub = dir.path().join("folder");
    fs::create_dir(&sub).unwrap();
    assert!(matches!(
        expect_failed(encrypt_one(&sub, PW, &opts(false))),
        Error::NotAFile(_)
    ));

    assert_eq!(dir_names(dir.path()), vec!["folder"]);
}

#[test]
fn already_encrypted_files_are_refused_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_file(dir.path(), "a.jpg", &pattern(5000));
    let output = expect_encrypted(encrypt_one(&src, PW, &opts(false)));
    let before = fs::read(&output).unwrap();

    let error = expect_failed(encrypt_one(&output, PW, &opts(false)));
    assert!(matches!(error, Error::AlreadyEncrypted));
    assert_eq!(fs::read(&output).unwrap(), before);
    assert_eq!(dir_names(dir.path()), vec!["a.jpg.vvenc"]);
}

#[test]
fn existing_destination_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let data = pattern(3000);
    let src = write_file(dir.path(), "b.png", &data);
    let existing = write_file(dir.path(), "b.png.vvenc", b"someone else's file");

    let error = expect_failed(encrypt_one(&src, PW, &opts(false)));
    assert!(matches!(error, Error::DestinationExists(_)));
    assert_eq!(fs::read(&src).unwrap(), data);
    assert_eq!(fs::read(&existing).unwrap(), b"someone else's file");
    assert_eq!(dir_names(dir.path()), vec!["b.png", "b.png.vvenc"]);
}

#[test]
fn weak_passwords_fail_before_touching_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let data = pattern(3000);
    let src = write_file(dir.path(), "c.png", &data);
    let error = expect_failed(encrypt_one(&src, "short", &opts(false)));
    assert!(matches!(error, Error::PasswordTooShort { .. }));
    assert_eq!(fs::read(&src).unwrap(), data);
    assert_eq!(dir_names(dir.path()), vec!["c.png"]);
}

#[cfg(unix)]
#[test]
fn read_only_files_fail_before_any_change() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let data = pattern(1000);
    let src = write_file(dir.path(), "ro.png", &data);
    fs::set_permissions(&src, fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(&src).is_ok() {
        return; // running with privileges that ignore permission bits
    }

    let error = expect_failed(encrypt_one(&src, PW, &opts(false)));
    assert!(matches!(error, Error::Io(_)));
    assert_eq!(fs::read(&src).unwrap(), data);
    assert_eq!(dir_names(dir.path()), vec!["ro.png"]);
}

// ---------- fault injection: file-state handling ----------

struct FailAfterVerify;

impl Hooks for FailAfterVerify {
    fn after_verify(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "simulated failure before the original is touched",
        ))
    }
}

struct FailAfterOverwrite;

impl Hooks for FailAfterOverwrite {
    fn after_overwrite(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "simulated failure after the original was overwritten",
        ))
    }
}

#[test]
fn failure_before_the_overwrite_leaves_the_original_intact_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let data = pattern(20_000);
    let src = write_file(dir.path(), "a.jpg", &data);

    let outcome = encrypt_one_with(&src, PW, &opts(false), &mut FailAfterVerify);

    assert!(matches!(expect_failed(outcome), Error::Io(_)));
    assert_eq!(fs::read(&src).unwrap(), data);
    assert_eq!(dir_names(dir.path()), vec!["a.jpg"]);
}

#[test]
fn failure_after_the_overwrite_is_reported_as_indeterminate_with_a_recovery_copy() {
    let dir = tempfile::tempdir().unwrap();
    let data = pattern(20_000);
    let src = write_file(dir.path(), "a.jpg", &data);

    let outcome = encrypt_one_with(&src, PW, &opts(false), &mut FailAfterOverwrite);

    let recovery = match &outcome {
        Outcome::Indeterminate {
            recovery_copy: Some(p),
            ..
        } => p.clone(),
        other => panic!("expected indeterminate with recovery copy, got {other:?}"),
    };
    assert!(recovery.exists());
    assert_eq!(
        decrypt_file(&recovery, PW),
        data,
        "recovery copy must decrypt to the original"
    );
    assert!(
        is_encrypted_file(&src),
        "original path now holds the ciphertext"
    );
    assert_eq!(fs::read(&src).unwrap(), fs::read(&recovery).unwrap());
    assert!(!dir.path().join("a.jpg.vvenc").exists());

    let report = FileReport {
        path: src.clone(),
        outcome,
    };
    let message = report.message();
    assert!(message.contains("a.jpg"));
    assert!(message.contains(&recovery.file_name().unwrap().to_string_lossy().into_owned()));
}

// ---------- batches ----------

#[test]
fn batch_reports_every_file_in_order_and_continues_after_failures() {
    let dir = tempfile::tempdir().unwrap();
    let d1 = pattern(1000);
    let d2 = pattern(2000);
    let one = write_file(dir.path(), "one.png", &d1);
    let missing = dir.path().join("missing.png");
    let two = write_file(dir.path(), "two.png", &d2);
    let paths = vec![one.clone(), missing.clone(), two.clone()];

    let mut calls: Vec<(usize, usize, PathBuf)> = Vec::new();
    let report = encrypt_batch(&paths, PW, &opts(false), &mut |i, n, p| {
        calls.push((i, n, p.to_path_buf()))
    })
    .unwrap();

    assert_eq!(report.files.len(), 3);
    assert_eq!(report.files[0].path, one);
    assert_eq!(report.files[1].path, missing);
    assert_eq!(report.files[2].path, two);
    assert_eq!(report.succeeded(), 2);
    assert_eq!(report.failed(), 1);
    assert_eq!(report.indeterminate(), 0);
    assert!(!report.all_succeeded());
    assert_eq!(
        calls,
        vec![
            (0, 3, one.clone()),
            (1, 3, missing.clone()),
            (2, 3, two.clone())
        ]
    );

    assert_eq!(decrypt_file(&dir.path().join("one.png.vvenc"), PW), d1);
    assert_eq!(decrypt_file(&dir.path().join("two.png.vvenc"), PW), d2);
}

#[test]
fn batch_success_helpers_and_per_file_independence() {
    let dir = tempfile::tempdir().unwrap();
    let same = pattern(4000);
    let a = write_file(dir.path(), "a.bin", &same);
    let b = write_file(dir.path(), "b.bin", &same);

    let report = encrypt_batch(&[a, b], PW, &opts(false), &mut |_, _, _| {}).unwrap();
    assert!(report.all_succeeded());

    let out_a = dir.path().join("a.bin.vvenc");
    let out_b = dir.path().join("b.bin.vvenc");
    assert_ne!(fs::read(&out_a).unwrap(), fs::read(&out_b).unwrap());

    let ha = read_header(&mut fs::File::open(&out_a).unwrap()).unwrap();
    let hb = read_header(&mut fs::File::open(&out_b).unwrap()).unwrap();
    assert_ne!(ha.salt, hb.salt);
    assert_ne!(ha.wrapped_key, hb.wrapped_key);
    assert_ne!(ha.nonce_prefix, hb.nonce_prefix);

    // each file is independently decryptable with the batch password
    assert_eq!(decrypt_file(&out_a, PW), same);
    assert_eq!(decrypt_file(&out_b, PW), same);
}

#[test]
fn batch_with_an_invalid_password_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data = pattern(500);
    let a = write_file(dir.path(), "a.bin", &data);
    let result = encrypt_batch(
        std::slice::from_ref(&a),
        "short",
        &opts(false),
        &mut |_, _, _| {},
    );
    assert!(matches!(result, Err(Error::PasswordTooShort { .. })));
    assert_eq!(fs::read(&a).unwrap(), data);
    assert_eq!(dir_names(dir.path()), vec!["a.bin"]);
}

#[test]
fn empty_batch_is_rejected_rather_than_silently_succeeding() {
    let result = encrypt_batch(&[], PW, &opts(false), &mut |_, _, _| {});
    assert!(matches!(result, Err(Error::InvalidArgument(_))));
}

#[test]
fn batch_marks_directories_as_failed_instead_of_skipping_them() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("folder");
    fs::create_dir(&sub).unwrap();
    let report = encrypt_batch(
        std::slice::from_ref(&sub),
        PW,
        &opts(false),
        &mut |_, _, _| {},
    )
    .unwrap();
    assert_eq!(report.files.len(), 1);
    assert!(matches!(
        report.files[0].outcome,
        Outcome::Failed {
            error: Error::NotAFile(_)
        }
    ));
}

#[test]
fn report_messages_identify_the_affected_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_file(dir.path(), "photo.jpg", &pattern(100));
    let ok = FileReport {
        path: src.clone(),
        outcome: encrypt_one(&src, PW, &opts(false)),
    };
    assert!(ok.message().contains("photo.jpg"));

    let missing = dir.path().join("gone.png");
    let failed = FileReport {
        path: missing.clone(),
        outcome: encrypt_one(&missing, PW, &opts(false)),
    };
    assert!(failed.message().contains("gone.png"));
}
