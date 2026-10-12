#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::*;
use crate::crypto::{encrypt_stream, EncryptOptions};
use crate::error::Error;
use crate::format::KdfParams;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const PW1: &str = "fixture-session-one";
const PW2: &str = "fixture-session-two";

fn opts() -> EncryptOptions {
    EncryptOptions {
        kdf: KdfParams {
            memory_kib: KDF_MEMORY_KIB_MIN,
            iterations: KDF_ITERATIONS_MIN,
            parallelism: KDF_PARALLELISM_MIN,
        },
        compress: false,
    }
}

fn encrypted_file(dir: &Path, name: &str, data: &[u8], password: &str) -> PathBuf {
    let path = dir.join(name);
    let mut file = fs::File::create(&path).unwrap();
    encrypt_stream(&mut Cursor::new(data), &mut file, password, &opts()).unwrap();
    path
}

// ---------- PasswordStore ----------

#[test]
fn password_store_starts_empty() {
    let store = PasswordStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    assert!(store.candidates().is_empty());
}

#[test]
fn passwords_are_returned_most_recent_first_without_duplicates() {
    let mut store = PasswordStore::new();
    assert!(store.remember("first-password"));
    assert!(store.remember("second-password"));
    assert_eq!(store.candidates(), vec!["second-password", "first-password"]);

    assert!(store.remember("first-password"));
    assert_eq!(store.candidates(), vec!["first-password", "second-password"]);
    assert_eq!(store.len(), 2);
}

#[test]
fn empty_passwords_are_never_retained() {
    let mut store = PasswordStore::new();
    assert!(!store.remember(""));
    assert!(store.is_empty());
}

#[test]
fn password_store_is_bounded_and_evicts_the_oldest() {
    let mut store = PasswordStore::new();
    for i in 0..MAX_SESSION_PASSWORDS + 2 {
        store.remember(&format!("password-number-{i}"));
    }
    assert_eq!(store.len(), MAX_SESSION_PASSWORDS);
    let newest = format!("password-number-{}", MAX_SESSION_PASSWORDS + 1);
    assert_eq!(store.candidates()[0], newest);
    assert!(!store.candidates().contains(&"password-number-0"));
    assert!(!store.candidates().contains(&"password-number-1"));
}

#[test]
fn forgetting_removes_every_password() {
    let mut store = PasswordStore::new();
    store.remember("some-password");
    store.remember("other-password");
    store.forget_all();
    assert!(store.is_empty());
    assert!(store.candidates().is_empty());
}

#[test]
fn password_store_debug_output_never_reveals_passwords() {
    let mut store = PasswordStore::new();
    store.remember("super-secret-fixture");
    let rendered = format!("{store:?}");
    assert!(!rendered.contains("super-secret-fixture"));
}

// ---------- DecryptedCache ----------

#[test]
fn cache_stores_and_returns_entries() {
    let mut cache = DecryptedCache::new(100);
    assert!(cache.is_empty());
    assert!(cache.insert(PathBuf::from("/a"), vec![1, 2, 3]));
    assert_eq!(cache.get(Path::new("/a")), Some(&[1u8, 2, 3][..]));
    assert_eq!(cache.get(Path::new("/missing")), None);
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.total_bytes(), 3);
    assert_eq!(cache.max_bytes(), 100);
}

#[test]
fn replacing_an_entry_updates_the_byte_total() {
    let mut cache = DecryptedCache::new(100);
    cache.insert(PathBuf::from("/a"), vec![0; 10]);
    cache.insert(PathBuf::from("/a"), vec![0; 4]);
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.total_bytes(), 4);
}

#[test]
fn oversized_entries_are_not_cached_and_do_not_evict_others() {
    let mut cache = DecryptedCache::new(10);
    cache.insert(PathBuf::from("/a"), vec![0; 6]);
    assert!(!cache.insert(PathBuf::from("/big"), vec![0; 11]));
    assert_eq!(cache.len(), 1);
    assert!(cache.get(Path::new("/a")).is_some());
    assert!(cache.get(Path::new("/big")).is_none());
}

#[test]
fn least_recently_used_entries_are_evicted_first() {
    let mut cache = DecryptedCache::new(10);
    cache.insert(PathBuf::from("/a"), vec![0; 4]);
    cache.insert(PathBuf::from("/b"), vec![0; 4]);
    cache.insert(PathBuf::from("/c"), vec![0; 4]);
    assert!(cache.get(Path::new("/a")).is_none());
    assert!(cache.get(Path::new("/b")).is_some());
    assert!(cache.get(Path::new("/c")).is_some());
    assert_eq!(cache.total_bytes(), 8);
}

#[test]
fn reading_an_entry_refreshes_its_recency() {
    let mut cache = DecryptedCache::new(10);
    cache.insert(PathBuf::from("/a"), vec![0; 4]);
    cache.insert(PathBuf::from("/b"), vec![0; 4]);
    assert!(cache.get(Path::new("/a")).is_some());
    cache.insert(PathBuf::from("/c"), vec![0; 4]);
    assert!(cache.get(Path::new("/b")).is_none());
    assert!(cache.get(Path::new("/a")).is_some());
    assert!(cache.get(Path::new("/c")).is_some());
}

#[test]
fn removing_and_clearing_release_all_accounted_bytes() {
    let mut cache = DecryptedCache::new(100);
    cache.insert(PathBuf::from("/a"), vec![0; 10]);
    cache.insert(PathBuf::from("/b"), vec![0; 20]);
    assert!(cache.remove(Path::new("/a")));
    assert!(!cache.remove(Path::new("/a")));
    assert_eq!(cache.total_bytes(), 20);
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.total_bytes(), 0);
    assert!(cache.get(Path::new("/b")).is_none());
}

// ---------- Session ----------

#[test]
fn new_session_uses_the_default_cache_policy() {
    let session = Session::new();
    assert!(session.passwords.is_empty());
    assert!(session.cache.is_empty());
    assert_eq!(session.cache.max_bytes(), DECRYPTED_CACHE_MAX_BYTES);
}

#[test]
fn privacy_actions_are_independent_and_shutdown_clears_everything() {
    let mut session = Session::new();
    session.passwords.remember(PW1);
    session.cache.insert(PathBuf::from("/a"), vec![1, 2, 3]);

    session.forget_password();
    assert!(session.passwords.is_empty());
    assert_eq!(session.cache.len(), 1);

    session.passwords.remember(PW1);
    session.clear_cache();
    assert!(session.cache.is_empty());
    assert_eq!(session.passwords.len(), 1);

    session.cache.insert(PathBuf::from("/b"), vec![9]);
    session.shutdown();
    assert!(session.passwords.is_empty());
    assert!(session.cache.is_empty());
}

#[test]
fn unlock_requires_a_password_when_none_is_available() {
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted_file(dir.path(), "a.jpg.vvenc", b"hello world", PW1);
    let mut session = Session::new();
    assert!(matches!(session.unlock(&path, None), Err(Error::PasswordRequired)));
}

#[test]
fn unlock_with_a_new_password_remembers_it_for_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let data = b"decrypt me please".to_vec();
    let path = encrypted_file(dir.path(), "a.jpg.vvenc", &data, PW1);
    let mut session = Session::new();

    let mut dec = session.unlock(&path, Some(PW1)).unwrap();
    assert_eq!(dec.read_plaintext(1 << 20).unwrap().as_slice(), data.as_slice());
    assert_eq!(session.passwords.candidates(), vec![PW1]);

    let mut again = session.unlock(&path, None).unwrap();
    assert_eq!(again.read_plaintext(1 << 20).unwrap().as_slice(), data.as_slice());
}

#[test]
fn wrong_password_is_rejected_not_remembered_and_leaves_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted_file(dir.path(), "a.jpg.vvenc", b"payload", PW1);
    let before = fs::read(&path).unwrap();
    let mut session = Session::new();

    assert!(matches!(session.unlock(&path, Some(PW2)), Err(Error::WrongPassword)));
    assert!(session.passwords.is_empty());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn retained_passwords_that_do_not_match_still_require_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted_file(dir.path(), "a.jpg.vvenc", b"payload", PW1);
    let mut session = Session::new();
    session.passwords.remember(PW2);
    assert!(matches!(session.unlock(&path, None), Err(Error::PasswordRequired)));
}

#[test]
fn forgetting_the_password_requires_re_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted_file(dir.path(), "a.jpg.vvenc", b"payload", PW1);
    let mut session = Session::new();
    session.unlock(&path, Some(PW1)).unwrap();
    session.forget_password();
    assert!(matches!(session.unlock(&path, None), Err(Error::PasswordRequired)));
}

#[test]
fn files_from_different_batches_open_with_their_own_remembered_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let first = encrypted_file(dir.path(), "one.bin.vvenc", b"first batch", PW1);
    let second = encrypted_file(dir.path(), "two.bin.vvenc", b"second batch", PW2);
    let mut session = Session::new();
    session.unlock(&first, Some(PW1)).unwrap();
    session.unlock(&second, Some(PW2)).unwrap();

    assert_eq!(
        session.unlock(&first, None).unwrap().read_plaintext(1 << 20).unwrap().as_slice(),
        b"first batch"
    );
    assert_eq!(
        session.unlock(&second, None).unwrap().read_plaintext(1 << 20).unwrap().as_slice(),
        b"second batch"
    );
    assert_eq!(session.passwords.len(), 2);
}

#[test]
fn unlock_rejects_files_that_are_not_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("plain.txt");
    fs::write(&plain, b"just text").unwrap();
    let mut session = Session::new();
    assert!(matches!(session.unlock(&plain, Some(PW1)), Err(Error::NotEncrypted)));
    assert!(matches!(session.unlock(&plain, None), Err(Error::NotEncrypted)));
    assert!(session.passwords.is_empty());
}