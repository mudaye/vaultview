#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::*;
use crate::error::Error;
use std::io::Cursor;

fn fast_kdf() -> KdfParams {
    KdfParams {
        memory_kib: KDF_MEMORY_KIB_MIN,
        iterations: KDF_ITERATIONS_MIN,
        parallelism: KDF_PARALLELISM_MIN,
    }
}

fn sample_header() -> Header {
    Header {
        version: FORMAT_VERSION,
        compression: COMPRESSION_NONE,
        chunk_size_log2: CHUNK_SIZE_LOG2,
        kdf: fast_kdf(),
        salt: [1u8; SALT_LEN],
        wrap_nonce: [2u8; NONCE_LEN],
        wrapped_key: [3u8; WRAPPED_KEY_LEN],
        nonce_prefix: [4u8; NONCE_PREFIX_LEN],
    }
}

fn sample_bytes() -> Vec<u8> {
    sample_header().to_bytes().to_vec()
}

fn with_byte(index: usize, value: u8) -> Vec<u8> {
    let mut b = sample_bytes();
    b[index] = value;
    b
}

fn with_u32(offset: usize, value: u32) -> Vec<u8> {
    let mut b = sample_bytes();
    b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    b
}

#[test]
fn serialized_header_has_the_documented_layout() {
    let b = sample_header().to_bytes();
    assert_eq!(b.len(), HEADER_LEN);
    assert_eq!(&b[0..8], &MAGIC);
    assert_eq!(&b[8..10], &FORMAT_VERSION.to_le_bytes());
    assert_eq!(b[10], KDF_ID_ARGON2ID);
    assert_eq!(b[11], AEAD_ID_XCHACHA20_POLY1305);
    assert_eq!(b[12], COMPRESSION_NONE);
    assert_eq!(b[13], CHUNK_SIZE_LOG2);
    assert_eq!(&b[14..18], &KDF_MEMORY_KIB_MIN.to_le_bytes());
    assert_eq!(&b[18..22], &KDF_ITERATIONS_MIN.to_le_bytes());
    assert_eq!(&b[22..26], &KDF_PARALLELISM_MIN.to_le_bytes());
    assert_eq!(&b[26..42], &[1u8; SALT_LEN]);
    assert_eq!(&b[42..66], &[2u8; NONCE_LEN]);
    assert_eq!(&b[66..114], &[3u8; WRAPPED_KEY_LEN]);
    assert_eq!(&b[114..129], &[4u8; NONCE_PREFIX_LEN]);
}

#[test]
fn header_round_trips() {
    let header = sample_header();
    let parsed = Header::from_bytes(&header.to_bytes()).unwrap();
    assert_eq!(parsed, header);
}

#[test]
fn trailing_bytes_after_the_header_are_ignored() {
    let mut bytes = sample_bytes();
    bytes.extend_from_slice(b"ciphertext follows");
    assert_eq!(Header::from_bytes(&bytes).unwrap(), sample_header());
}

#[test]
fn wrong_magic_is_not_encrypted() {
    assert!(matches!(Header::from_bytes(&[]), Err(Error::NotEncrypted)));
    assert!(matches!(
        Header::from_bytes(b"short"),
        Err(Error::NotEncrypted)
    ));
    assert!(matches!(
        Header::from_bytes(&with_byte(0, b'X')),
        Err(Error::NotEncrypted)
    ));
    assert!(matches!(
        Header::from_bytes(&[0u8; HEADER_LEN]),
        Err(Error::NotEncrypted)
    ));
}

#[test]
fn bad_magic_takes_precedence_over_a_bad_version() {
    let mut bytes = sample_bytes();
    bytes[0] = 0;
    bytes[8] = 99;
    assert!(matches!(
        Header::from_bytes(&bytes),
        Err(Error::NotEncrypted)
    ));
}

#[test]
fn truncated_header_with_valid_magic_is_invalid() {
    let bytes = sample_bytes();
    assert!(matches!(
        Header::from_bytes(&bytes[..HEADER_LEN - 1]),
        Err(Error::InvalidHeader(_))
    ));
    assert!(matches!(
        Header::from_bytes(&bytes[..MAGIC.len()]),
        Err(Error::InvalidHeader(_))
    ));
}

#[test]
fn unsupported_versions_are_reported_with_their_number() {
    for version in [0u16, 2, 7, u16::MAX] {
        let mut bytes = sample_bytes();
        bytes[8..10].copy_from_slice(&version.to_le_bytes());
        match Header::from_bytes(&bytes) {
            Err(Error::UnsupportedVersion(v)) => assert_eq!(v, version),
            other => panic!("version {version}: unexpected result {other:?}"),
        }
    }
}

#[test]
fn unknown_algorithm_and_compression_identifiers_are_rejected() {
    assert!(matches!(
        Header::from_bytes(&with_byte(10, 9)),
        Err(Error::InvalidHeader(_))
    ));
    assert!(matches!(
        Header::from_bytes(&with_byte(11, 9)),
        Err(Error::InvalidHeader(_))
    ));
    assert!(matches!(
        Header::from_bytes(&with_byte(12, 7)),
        Err(Error::InvalidHeader(_))
    ));
}

#[test]
fn chunk_size_exponent_outside_bounds_is_rejected() {
    let too_small = CHUNK_SIZE_LOG2_MIN - 1;
    let too_large = CHUNK_SIZE_LOG2_MAX + 1;
    assert!(matches!(
        Header::from_bytes(&with_byte(13, too_small)),
        Err(Error::InvalidHeader(_))
    ));
    assert!(matches!(
        Header::from_bytes(&with_byte(13, too_large)),
        Err(Error::InvalidHeader(_))
    ));
    let ok = Header::from_bytes(&with_byte(13, CHUNK_SIZE_LOG2_MIN)).unwrap();
    assert_eq!(ok.chunk_size(), 1usize << CHUNK_SIZE_LOG2_MIN);
}

#[test]
fn kdf_parameters_outside_bounds_are_rejected() {
    assert!(matches!(
        Header::from_bytes(&with_u32(14, 0)),
        Err(Error::InvalidKdfParams)
    ));
    assert!(matches!(
        Header::from_bytes(&with_u32(14, KDF_MEMORY_KIB_MAX + 1)),
        Err(Error::InvalidKdfParams)
    ));
    assert!(matches!(
        Header::from_bytes(&with_u32(18, 0)),
        Err(Error::InvalidKdfParams)
    ));
    assert!(matches!(
        Header::from_bytes(&with_u32(18, KDF_ITERATIONS_MAX + 1)),
        Err(Error::InvalidKdfParams)
    ));
    assert!(matches!(
        Header::from_bytes(&with_u32(22, 0)),
        Err(Error::InvalidKdfParams)
    ));
    assert!(matches!(
        Header::from_bytes(&with_u32(22, KDF_PARALLELISM_MAX + 1)),
        Err(Error::InvalidKdfParams)
    ));
    assert!(matches!(
        Header::from_bytes(&with_u32(14, u32::MAX)),
        Err(Error::InvalidKdfParams)
    ));
}

#[test]
fn default_kdf_params_match_constants_and_validate() {
    let d = KdfParams::default();
    assert_eq!(d.memory_kib, KDF_MEMORY_KIB_DEFAULT);
    assert_eq!(d.iterations, KDF_ITERATIONS_DEFAULT);
    assert_eq!(d.parallelism, KDF_PARALLELISM_DEFAULT);
    assert!(d.validate().is_ok());
    assert!(fast_kdf().validate().is_ok());
    let bad = KdfParams {
        memory_kib: 0,
        ..fast_kdf()
    };
    assert!(matches!(bad.validate(), Err(Error::InvalidKdfParams)));
}

#[test]
fn key_wrap_aad_covers_parameters_and_salt_but_not_the_wrapped_key() {
    let base = sample_header();
    let aad = base.key_wrap_aad();
    assert_eq!(aad.len(), KEY_WRAP_AAD_LEN);
    assert_eq!(aad.as_slice(), &base.to_bytes()[..KEY_WRAP_AAD_LEN]);

    let mut salt_changed = base.clone();
    salt_changed.salt[0] ^= 1;
    assert_ne!(salt_changed.key_wrap_aad(), aad);

    let mut compression_changed = base.clone();
    compression_changed.compression = COMPRESSION_DEFLATE;
    assert_ne!(compression_changed.key_wrap_aad(), aad);

    let mut key_changed = base.clone();
    key_changed.wrapped_key[0] ^= 1;
    assert_eq!(key_changed.key_wrap_aad(), aad);
}

#[test]
fn chunk_aad_is_the_whole_serialized_header() {
    let h = sample_header();
    assert_eq!(h.chunk_aad(), h.to_bytes());
    let mut other = h.clone();
    other.nonce_prefix[0] ^= 1;
    assert_ne!(other.chunk_aad(), h.chunk_aad());
}

#[test]
fn chunk_size_follows_the_header_exponent() {
    assert_eq!(sample_header().chunk_size(), CHUNK_SIZE);
}

#[test]
fn prefix_recognition_depends_only_on_magic() {
    let mut data = MAGIC.to_vec();
    data.extend_from_slice(b"anything");
    assert!(is_encrypted_prefix(&data));
    assert!(is_encrypted_prefix(&MAGIC));
    assert!(!is_encrypted_prefix(&MAGIC[..7]));
    assert!(!is_encrypted_prefix(b""));
    assert!(!is_encrypted_prefix(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn read_header_parses_streams_and_reports_errors() {
    let mut ok = Cursor::new(sample_bytes());
    assert_eq!(read_header(&mut ok).unwrap(), sample_header());

    let mut not_ours = Cursor::new(b"just some text, not an encrypted file at all".to_vec());
    assert!(matches!(
        read_header(&mut not_ours),
        Err(Error::NotEncrypted)
    ));

    let mut short = Cursor::new(sample_bytes()[..HEADER_LEN - 10].to_vec());
    assert!(matches!(
        read_header(&mut short),
        Err(Error::InvalidHeader(_))
    ));

    let mut empty = Cursor::new(Vec::new());
    assert!(matches!(read_header(&mut empty), Err(Error::NotEncrypted)));
}

#[test]
fn file_recognition_ignores_the_extension() {
    let dir = tempfile::tempdir().unwrap();

    let disguised = dir.path().join("looks_like_a_photo.jpg");
    std::fs::write(&disguised, sample_bytes()).unwrap();
    assert!(is_encrypted_file(&disguised));

    let impostor = dir.path().join("claims_to_be_encrypted.vvenc");
    std::fs::write(&impostor, b"plain text").unwrap();
    assert!(!is_encrypted_file(&impostor));

    assert!(!is_encrypted_file(&dir.path().join("does_not_exist")));
    assert!(!is_encrypted_file(dir.path()));
}

#[test]
fn payload_length_is_derived_from_the_file_length() {
    let cs = CHUNK_SIZE;
    let stride = (CHUNK_SIZE + TAG_LEN) as u64;
    let h = HEADER_LEN as u64;
    let t = TAG_LEN as u64;

    assert_eq!(payload_len(h + t, cs).unwrap(), 0);
    assert_eq!(payload_len(h + t + 100, cs).unwrap(), 100);
    assert_eq!(payload_len(h + stride, cs).unwrap(), CHUNK_SIZE as u64);
    assert_eq!(
        payload_len(h + stride + t + 10, cs).unwrap(),
        CHUNK_SIZE as u64 + 10
    );
    assert_eq!(
        payload_len(h + 3 * stride, cs).unwrap(),
        3 * CHUNK_SIZE as u64
    );
}

#[test]
fn impossible_file_lengths_are_corrupted() {
    let cs = CHUNK_SIZE;
    let stride = (CHUNK_SIZE + TAG_LEN) as u64;
    let h = HEADER_LEN as u64;
    let t = TAG_LEN as u64;

    assert!(matches!(payload_len(0, cs), Err(Error::Corrupted)));
    assert!(matches!(payload_len(h - 1, cs), Err(Error::Corrupted)));
    assert!(matches!(payload_len(h, cs), Err(Error::Corrupted)));
    assert!(matches!(payload_len(h + t - 1, cs), Err(Error::Corrupted)));
    assert!(matches!(
        payload_len(h + stride + 3, cs),
        Err(Error::Corrupted)
    ));
}
