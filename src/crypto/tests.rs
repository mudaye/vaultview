#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::*;
use crate::error::Error;
use crate::format::{Header, KdfParams};
use std::io::{Cursor, Read, Seek, SeekFrom};

const PW: &str = "fixture-password-one";
const OTHER_PW: &str = "fixture-password-two";

fn fast_kdf() -> KdfParams {
    KdfParams {
        memory_kib: KDF_MEMORY_KIB_MIN,
        iterations: KDF_ITERATIONS_MIN,
        parallelism: KDF_PARALLELISM_MIN,
    }
}

fn opts(compress: bool) -> EncryptOptions {
    EncryptOptions { kdf: fast_kdf(), compress }
}

fn pattern(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        })
        .collect()
}

fn encrypt(data: &[u8], compress: bool) -> Vec<u8> {
    let mut out = Vec::new();
    encrypt_stream(&mut Cursor::new(data), &mut out, PW, &opts(compress)).unwrap();
    out
}

fn decrypt(ciphertext: &[u8], password: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    decrypt_stream(Cursor::new(ciphertext), &mut out, password)?;
    Ok(out)
}

fn flipped(ciphertext: &[u8], index: usize) -> Vec<u8> {
    let mut v = ciphertext.to_vec();
    v[index] ^= 0x01;
    v
}

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("simulated read failure"))
    }
}

struct OneByteReader<R>(R);

impl<R: Read> Read for OneByteReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(1);
        self.0.read(&mut buf[..n])
    }
}

// ---------- key derivation and key wrapping ----------

#[test]
fn kek_derivation_is_deterministic_and_input_sensitive() {
    let salt = [9u8; SALT_LEN];
    let a = derive_kek(PW, &salt, &fast_kdf()).unwrap();
    let b = derive_kek(PW, &salt, &fast_kdf()).unwrap();
    assert_eq!(*a, *b);

    let other_password = derive_kek(OTHER_PW, &salt, &fast_kdf()).unwrap();
    assert_ne!(*a, *other_password);

    let other_salt = derive_kek(PW, &[8u8; SALT_LEN], &fast_kdf()).unwrap();
    assert_ne!(*a, *other_salt);

    let other_params = KdfParams { iterations: 2, ..fast_kdf() };
    let changed = derive_kek(PW, &salt, &other_params).unwrap();
    assert_ne!(*a, *changed);
}

#[test]
fn kek_derivation_rejects_empty_password_and_invalid_params() {
    let salt = [9u8; SALT_LEN];
    assert!(matches!(derive_kek("", &salt, &fast_kdf()), Err(Error::EmptyPassword)));
    let bad = KdfParams { memory_kib: 0, ..fast_kdf() };
    assert!(matches!(derive_kek(PW, &salt, &bad), Err(Error::InvalidKdfParams)));
}

#[test]
fn file_key_wrapping_round_trips_and_authenticates() {
    let kek = [7u8; KEY_LEN];
    let nonce = [9u8; NONCE_LEN];
    let key = [5u8; KEY_LEN];

    let wrapped = wrap_file_key(&kek, &nonce, b"aad", &key).unwrap();
    assert_eq!(wrapped.len(), WRAPPED_KEY_LEN);
    assert_ne!(&wrapped[..KEY_LEN], &key[..]);

    let unwrapped = unwrap_file_key(&kek, &nonce, b"aad", &wrapped).unwrap();
    assert_eq!(*unwrapped, key);

    let wrong_kek = [8u8; KEY_LEN];
    assert!(matches!(unwrap_file_key(&wrong_kek, &nonce, b"aad", &wrapped), Err(Error::WrongPassword)));
    assert!(matches!(unwrap_file_key(&kek, &nonce, b"other", &wrapped), Err(Error::WrongPassword)));
    assert!(matches!(
        unwrap_file_key(&kek, &[1u8; NONCE_LEN], b"aad", &wrapped),
        Err(Error::WrongPassword)
    ));

    let mut tampered = wrapped;
    tampered[3] ^= 1;
    assert!(matches!(unwrap_file_key(&kek, &nonce, b"aad", &tampered), Err(Error::WrongPassword)));
}

#[test]
fn chunk_nonce_encodes_prefix_index_and_final_flag() {
    let prefix = [0xABu8; NONCE_PREFIX_LEN];
    let n = chunk_nonce(&prefix, 0x0102_0304_0506_0708, true);
    assert_eq!(&n[..NONCE_PREFIX_LEN], &prefix);
    assert_eq!(&n[NONCE_PREFIX_LEN..NONCE_PREFIX_LEN + 8], &0x0102_0304_0506_0708u64.to_be_bytes());
    assert_eq!(n[NONCE_LEN - 1], 1);

    let not_last = chunk_nonce(&prefix, 0x0102_0304_0506_0708, false);
    assert_eq!(not_last[NONCE_LEN - 1], 0);
    assert_ne!(n, not_last);
    assert_ne!(chunk_nonce(&prefix, 1, false), chunk_nonce(&prefix, 2, false));
}

// ---------- round trips ----------

#[test]
fn round_trips_across_chunk_boundaries() {
    for n in [0usize, 1, 100, CHUNK_SIZE - 1, CHUNK_SIZE, CHUNK_SIZE + 1, 2 * CHUNK_SIZE, 2 * CHUNK_SIZE + 17] {
        let data = pattern(n);
        let ct = encrypt(&data, false);
        let chunks = if n == 0 { 1 } else { n.div_ceil(CHUNK_SIZE) };
        assert_eq!(ct.len(), HEADER_LEN + n + chunks * TAG_LEN, "ciphertext size for {n}");
        assert_eq!(decrypt(&ct, PW).unwrap(), data, "round trip for {n}");
    }
}

#[test]
fn empty_input_produces_a_valid_authenticated_file() {
    let ct = encrypt(&[], false);
    assert_eq!(ct.len(), HEADER_LEN + TAG_LEN);
    let mut out = Vec::new();
    let written = decrypt_stream(Cursor::new(&ct), &mut out, PW).unwrap();
    assert_eq!(written, 0);
    assert!(out.is_empty());
}

#[test]
fn unicode_passwords_work() {
    let password = "pässwörd-日本語-🔑";
    let data = pattern(5000);
    let mut ct = Vec::new();
    encrypt_stream(&mut Cursor::new(&data), &mut ct, password, &opts(false)).unwrap();
    assert_eq!(decrypt(&ct, password).unwrap(), data);
    assert!(matches!(decrypt(&ct, "passwörd-日本語-🔑"), Err(Error::WrongPassword)));
}

#[test]
fn encrypt_reports_accurate_statistics() {
    let data = pattern(70_000);
    let mut out = Vec::new();
    let stats = encrypt_stream(&mut Cursor::new(&data), &mut out, PW, &opts(false)).unwrap();
    assert_eq!(stats.plaintext_len, 70_000);
    assert_eq!(stats.stored_len, 70_000);
    assert_eq!(stats.compression, COMPRESSION_NONE);
    assert_eq!(stats.ciphertext_len, out.len() as u64);
}

#[test]
fn header_of_output_is_valid_and_recognised() {
    let ct = encrypt(&pattern(1000), false);
    assert!(crate::format::is_encrypted_prefix(&ct));
    let header = Header::from_bytes(&ct).unwrap();
    assert_eq!(header.version, FORMAT_VERSION);
    assert_eq!(header.compression, COMPRESSION_NONE);
    assert_eq!(header.kdf, fast_kdf());
}

#[test]
fn plaintext_does_not_appear_in_ciphertext() {
    let data = vec![b'Q'; 4096];
    let ct = encrypt(&data, false);
    assert!(!ct.windows(64).any(|w| w == &data[..64]));
}

#[test]
fn two_encryptions_of_the_same_input_differ_in_salt_key_and_nonce() {
    let data = pattern(3000);
    let a = encrypt(&data, false);
    let b = encrypt(&data, false);
    assert_ne!(a, b);
    let ha = Header::from_bytes(&a).unwrap();
    let hb = Header::from_bytes(&b).unwrap();
    assert_ne!(ha.salt, hb.salt);
    assert_ne!(ha.wrapped_key, hb.wrapped_key);
    assert_ne!(ha.nonce_prefix, hb.nonce_prefix);
    assert_ne!(ha.wrap_nonce, hb.wrap_nonce);
}

#[test]
fn short_reads_from_the_source_do_not_change_the_result() {
    let data = pattern(70_000);
    let mut ct = Vec::new();
    encrypt_stream(&mut OneByteReader(Cursor::new(&data)), &mut ct, PW, &opts(false)).unwrap();
    assert_eq!(ct.len(), HEADER_LEN + 70_000 + 2 * TAG_LEN);
    assert_eq!(decrypt(&ct, PW).unwrap(), data);

    let text = vec![b'z'; 100_000];
    let mut ct2 = Vec::new();
    encrypt_stream(&mut OneByteReader(Cursor::new(&text)), &mut ct2, PW, &opts(true)).unwrap();
    assert_eq!(decrypt(&ct2, PW).unwrap(), text);
}

#[test]
fn source_read_errors_are_propagated() {
    let mut out = Vec::new();
    let result = encrypt_stream(&mut FailingReader, &mut out, PW, &opts(false));
    assert!(matches!(result, Err(Error::Io(_))));
}

// ---------- password handling ----------

#[test]
fn empty_password_is_rejected_for_encryption_and_decryption() {
    let mut out = Vec::new();
    let result = encrypt_stream(&mut Cursor::new(b"data".as_slice()), &mut out, "", &opts(false));
    assert!(matches!(result, Err(Error::EmptyPassword)));

    let ct = encrypt(b"data", false);
    assert!(matches!(decrypt(&ct, ""), Err(Error::EmptyPassword)));
}

#[test]
fn wrong_password_is_rejected() {
    let ct = encrypt(&pattern(2000), false);
    assert!(matches!(decrypt(&ct, OTHER_PW), Err(Error::WrongPassword)));
    assert!(matches!(Decryptor::open(Cursor::new(ct), OTHER_PW), Err(Error::WrongPassword)));
}

// ---------- tampering and corruption ----------

#[test]
fn magic_and_version_tampering_is_reported_precisely() {
    let ct = encrypt(&pattern(1000), false);
    assert!(matches!(decrypt(&flipped(&ct, 0), PW), Err(Error::NotEncrypted)));
    let mut future = ct.clone();
    future[8] = 2;
    assert!(matches!(decrypt(&future, PW), Err(Error::UnsupportedVersion(2))));
    let mut bad_kdf_id = ct.clone();
    bad_kdf_id[10] = 9;
    assert!(matches!(decrypt(&bad_kdf_id, PW), Err(Error::InvalidHeader(_))));
    let mut bad_aead_id = ct.clone();
    bad_aead_id[11] = 9;
    assert!(matches!(decrypt(&bad_aead_id, PW), Err(Error::InvalidHeader(_))));
}

#[test]
fn out_of_range_kdf_parameters_are_rejected_before_deriving() {
    let ct = encrypt(&pattern(1000), false);
    let mut zero_iterations = ct.clone();
    zero_iterations[18..22].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(decrypt(&zero_iterations, PW), Err(Error::InvalidKdfParams)));

    let mut huge_memory = ct.clone();
    huge_memory[14..18].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(decrypt(&huge_memory, PW), Err(Error::InvalidKdfParams)));
}

#[test]
fn authenticated_header_fields_reject_modification() {
    let ct = encrypt(&pattern(1000), false);
    // compression flag, chunk exponent, KDF memory, salt (first/last), wrap nonce (first/last),
    // wrapped key (first/last)
    for index in [12usize, 13, 14, 26, 41, 42, 65, 66, 113] {
        assert!(
            matches!(decrypt(&flipped(&ct, index), PW), Err(Error::WrongPassword)),
            "flipping header byte {index} must be rejected"
        );
    }
}

#[test]
fn nonce_prefix_tampering_is_detected_as_corruption() {
    let ct = encrypt(&pattern(1000), false);
    for index in [114usize, 120, 128] {
        assert!(matches!(decrypt(&flipped(&ct, index), PW), Err(Error::Corrupted)), "byte {index}");
    }
}

#[test]
fn ciphertext_and_tag_tampering_is_detected() {
    let data = pattern(2 * CHUNK_SIZE + 500);
    let ct = encrypt(&data, false);
    let indices = [
        HEADER_LEN,
        HEADER_LEN + 777,
        HEADER_LEN + CHUNK_SIZE + TAG_LEN - 1,
        HEADER_LEN + CHUNK_SIZE + TAG_LEN,
        ct.len() - 1,
    ];
    for index in indices {
        assert!(matches!(decrypt(&flipped(&ct, index), PW), Err(Error::Corrupted)), "byte {index}");
    }
}

#[test]
fn truncation_is_detected_even_at_chunk_boundaries() {
    let data = pattern(2 * CHUNK_SIZE + 500);
    let ct = encrypt(&data, false);
    let last_chunk = 500 + TAG_LEN;

    assert!(matches!(decrypt(&ct[..ct.len() - last_chunk], PW), Err(Error::Corrupted)));
    assert!(matches!(decrypt(&ct[..ct.len() - 1], PW), Err(Error::Corrupted)));
    assert!(matches!(decrypt(&ct[..HEADER_LEN + TAG_LEN], PW), Err(Error::Corrupted)));
    assert!(matches!(decrypt(&ct[..HEADER_LEN], PW), Err(Error::Corrupted)));
    assert!(matches!(decrypt(&ct[..HEADER_LEN - 1], PW), Err(Error::InvalidHeader(_))));
    assert!(matches!(decrypt(&ct[..5], PW), Err(Error::NotEncrypted)));
}

#[test]
fn appended_data_is_detected() {
    let partial = encrypt(&pattern(CHUNK_SIZE + 500), false);
    let mut extended = partial.clone();
    extended.extend_from_slice(b"12345");
    assert!(matches!(decrypt(&extended, PW), Err(Error::Corrupted)));

    let exact = encrypt(&pattern(2 * CHUNK_SIZE), false);
    for extra in [3usize, 20, 1000] {
        let mut bigger = exact.clone();
        bigger.extend(std::iter::repeat(0xEE).take(extra));
        assert!(matches!(decrypt(&bigger, PW), Err(Error::Corrupted)), "extra {extra}");
    }
}

#[test]
fn reordered_chunks_are_detected() {
    let data = pattern(2 * CHUNK_SIZE + 500);
    let ct = encrypt(&data, false);
    let stride = CHUNK_SIZE + TAG_LEN;
    let mut swapped = ct.clone();
    let (first, second) = (HEADER_LEN, HEADER_LEN + stride);
    swapped[first..first + stride].copy_from_slice(&ct[second..second + stride]);
    swapped[second..second + stride].copy_from_slice(&ct[first..first + stride]);
    assert!(matches!(decrypt(&swapped, PW), Err(Error::Corrupted)));
}

#[test]
fn chunks_cannot_be_transplanted_between_files() {
    let data = pattern(CHUNK_SIZE + 10);
    let a = encrypt(&data, false);
    let b = encrypt(&data, false);
    let stride = CHUNK_SIZE + TAG_LEN;
    let mut mixed = a.clone();
    mixed[HEADER_LEN..HEADER_LEN + stride].copy_from_slice(&b[HEADER_LEN..HEADER_LEN + stride]);
    assert!(matches!(decrypt(&mixed, PW), Err(Error::Corrupted)));
}

#[test]
fn random_access_reads_report_corruption_as_invalid_data() {
    let ct = encrypt(&pattern(3 * CHUNK_SIZE), false);
    let tampered = flipped(&ct, HEADER_LEN + 5);
    let mut dec = Decryptor::open(Cursor::new(tampered), PW).unwrap();
    let mut sink = Vec::new();
    let err = dec.read_to_end(&mut sink).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

// ---------- compression ----------

#[test]
fn should_compress_prefers_compressible_samples_only() {
    assert!(should_compress(&vec![0u8; 10_000]));
    assert!(should_compress(&b"hello hello hello hello ".repeat(500)));
    assert!(!should_compress(&pattern(10_000)));
    assert!(!should_compress(&[]));
}

#[test]
fn compressible_data_is_compressed_and_round_trips() {
    let data = vec![b'a'; 300_000];
    let mut ct = Vec::new();
    let stats = encrypt_stream(&mut Cursor::new(&data), &mut ct, PW, &opts(true)).unwrap();
    assert_eq!(stats.compression, COMPRESSION_DEFLATE);
    assert_eq!(stats.plaintext_len, 300_000);
    assert!(stats.stored_len < 10_000);
    assert!(ct.len() < 100_000);
    assert_eq!(Header::from_bytes(&ct).unwrap().compression, COMPRESSION_DEFLATE);
    assert_eq!(decrypt(&ct, PW).unwrap(), data);
}

#[test]
fn compression_is_skipped_when_disabled_or_not_beneficial() {
    let compressible = vec![b'a'; 300_000];
    let off = encrypt(&compressible, false);
    assert_eq!(Header::from_bytes(&off).unwrap().compression, COMPRESSION_NONE);

    let random = pattern(100_000);
    let skipped = encrypt(&random, true);
    assert_eq!(Header::from_bytes(&skipped).unwrap().compression, COMPRESSION_NONE);
    assert_eq!(skipped.len(), HEADER_LEN + 100_000 + 2 * TAG_LEN);

    let empty = encrypt(&[], true);
    assert_eq!(Header::from_bytes(&empty).unwrap().compression, COMPRESSION_NONE);
}

#[test]
fn compressed_files_detect_tampering() {
    let ct = encrypt(&vec![b'a'; 200_000], true);
    assert!(matches!(decrypt(&flipped(&ct, HEADER_LEN + 3), PW), Err(Error::Corrupted)));
    assert!(matches!(decrypt(&ct[..ct.len() - 1], PW), Err(Error::Corrupted)));
}

// ---------- Decryptor ----------

#[test]
fn decryptor_supports_random_access_over_the_payload() {
    let data = pattern(3 * CHUNK_SIZE + 123);
    let ct = encrypt(&data, false);
    let mut dec = Decryptor::open(Cursor::new(ct), PW).unwrap();
    assert_eq!(dec.payload_len(), data.len() as u64);
    assert!(!dec.is_compressed());
    assert_eq!(dec.header().compression, COMPRESSION_NONE);

    dec.seek(SeekFrom::Start(CHUNK_SIZE as u64 - 10)).unwrap();
    let mut across = [0u8; 20];
    dec.read_exact(&mut across).unwrap();
    assert_eq!(&across[..], &data[CHUNK_SIZE - 10..CHUNK_SIZE + 10]);

    dec.seek(SeekFrom::End(-5)).unwrap();
    let mut tail = Vec::new();
    dec.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, &data[data.len() - 5..]);

    assert_eq!(dec.seek(SeekFrom::End(0)).unwrap(), data.len() as u64);
    let mut eof = [0u8; 8];
    assert_eq!(dec.read(&mut eof).unwrap(), 0);

    dec.seek(SeekFrom::Start(100)).unwrap();
    dec.seek(SeekFrom::Current(2 * CHUNK_SIZE as i64)).unwrap();
    assert_eq!(dec.stream_position().unwrap(), 100 + 2 * CHUNK_SIZE as u64);
    let mut mid = [0u8; 16];
    dec.read_exact(&mut mid).unwrap();
    assert_eq!(&mid[..], &data[100 + 2 * CHUNK_SIZE..100 + 2 * CHUNK_SIZE + 16]);

    dec.seek(SeekFrom::Start(0)).unwrap();
    let mut all = Vec::new();
    dec.read_to_end(&mut all).unwrap();
    assert_eq!(all, data);
}

#[test]
fn seeking_before_the_start_is_an_error() {
    let ct = encrypt(&pattern(1000), false);
    let mut dec = Decryptor::open(Cursor::new(ct), PW).unwrap();
    let err = dec.seek(SeekFrom::Current(-1)).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(dec.seek(SeekFrom::End(-1001)).is_err());
}

#[test]
fn empty_payload_is_authenticated_on_first_read() {
    let ct = encrypt(&[], false);
    let mut dec = Decryptor::open(Cursor::new(ct.clone()), PW).unwrap();
    assert_eq!(dec.payload_len(), 0);
    let mut buf = [0u8; 4];
    assert_eq!(dec.read(&mut buf).unwrap(), 0);

    let tampered = flipped(&ct, ct.len() - 1);
    let mut bad = Decryptor::open(Cursor::new(tampered), PW).unwrap();
    assert_eq!(bad.read(&mut buf).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn read_plaintext_handles_compressed_and_plain_payloads() {
    let compressible = vec![b'x'; 50_000];
    let ct = encrypt(&compressible, true);
    let mut dec = Decryptor::open(Cursor::new(ct), PW).unwrap();
    assert!(dec.is_compressed());
    let plain = dec.read_plaintext(1 << 20).unwrap();
    assert_eq!(plain.as_slice(), compressible.as_slice());

    let random = pattern(40_000);
    let ct2 = encrypt(&random, false);
    let mut dec2 = Decryptor::open(Cursor::new(ct2), PW).unwrap();
    let plain2 = dec2.read_plaintext(1 << 20).unwrap();
    assert_eq!(plain2.as_slice(), random.as_slice());

    // read_plaintext always starts from the beginning, even after other reads
    dec2.seek(SeekFrom::Start(500)).unwrap();
    assert_eq!(dec2.read_plaintext(1 << 20).unwrap().as_slice(), random.as_slice());
}

#[test]
fn read_plaintext_enforces_the_size_limit() {
    let data = pattern(10_000);
    let ct = encrypt(&data, false);

    let mut under = Decryptor::open(Cursor::new(ct.clone()), PW).unwrap();
    assert!(matches!(under.read_plaintext(5_000), Err(Error::TooLarge { limit: 5_000 })));

    let mut exact = Decryptor::open(Cursor::new(ct), PW).unwrap();
    assert_eq!(exact.read_plaintext(10_000).unwrap().len(), 10_000);

    let bomb = encrypt(&vec![0u8; 2_000_000], true);
    let mut dec = Decryptor::open(Cursor::new(bomb), PW).unwrap();
    assert!(matches!(dec.read_plaintext(1_000), Err(Error::TooLarge { limit: 1_000 })));
}

#[test]
fn read_plaintext_reports_corruption() {
    let ct = encrypt(&vec![b'q'; 100_000], true);
    let tampered = flipped(&ct, HEADER_LEN + 10);
    let mut dec = Decryptor::open(Cursor::new(tampered), PW).unwrap();
    assert!(matches!(dec.read_plaintext(1 << 20), Err(Error::Corrupted)));

    let plain = encrypt(&pattern(CHUNK_SIZE + 5), false);
    let tampered_plain = flipped(&plain, HEADER_LEN + 10);
    let mut dec2 = Decryptor::open(Cursor::new(tampered_plain), PW).unwrap();
    assert!(matches!(dec2.read_plaintext(1 << 20), Err(Error::Corrupted)));
}