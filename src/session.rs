//! Session state: retained passwords and the decrypted-content cache.
//!
//! Nothing here is ever written to disk. Passwords and cached plaintext are
//! zeroized when released, on a best-effort basis (the runtime and operating
//! system may keep copies that this application cannot reach).

use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::constants::{DECRYPTED_CACHE_MAX_BYTES, MAX_SESSION_PASSWORDS};
use crate::crypto::Decryptor;
use crate::error::{Error, Result};
use crate::format;

/// Bounded, most-recently-used-first store of passwords for this session.
#[derive(Default)]
pub struct PasswordStore {
    items: Vec<Zeroizing<String>>,
}

impl fmt::Debug for PasswordStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PasswordStore")
            .field("count", &self.items.len())
            .finish()
    }
}

impl PasswordStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remembers a password (most recent first). Empty passwords are ignored.
    /// Returns whether the password was stored.
    pub fn remember(&mut self, password: &str) -> bool {
        if password.is_empty() {
            return false;
        }
        self.items.retain(|p| p.as_str() != password);
        self.items.insert(0, Zeroizing::new(password.to_string()));
        self.items.truncate(MAX_SESSION_PASSWORDS);
        true
    }

    /// Retained passwords, most recently used first.
    pub fn candidates(&self) -> Vec<&str> {
        self.items.iter().map(|p| p.as_str()).collect()
    }

    /// Forgets every retained password.
    pub fn forget_all(&mut self) {
        self.items.clear();
    }

    /// Number of retained passwords.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no password is retained.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

struct CacheEntry {
    path: PathBuf,
    data: Zeroizing<Vec<u8>>,
}

/// In-memory, size-bounded, least-recently-used cache of decrypted content.
pub struct DecryptedCache {
    entries: Vec<CacheEntry>,
    max_bytes: usize,
}

impl DecryptedCache {
    /// Creates an empty cache holding at most `max_bytes` bytes.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_bytes,
        }
    }

    /// Stores `data` for `path`, evicting the least recently used entries.
    /// Returns `false` (caching nothing for that path) when `data` alone
    /// exceeds the limit.
    pub fn insert(&mut self, path: PathBuf, data: Vec<u8>) -> bool {
        self.remove(&path);
        if data.len() > self.max_bytes {
            return false;
        }
        while !self.entries.is_empty() && self.total_bytes() + data.len() > self.max_bytes {
            self.entries.remove(0);
        }
        self.entries.push(CacheEntry {
            path,
            data: Zeroizing::new(data),
        });
        true
    }

    /// Returns the cached data for `path` and marks it most recently used.
    pub fn get(&mut self, path: &Path) -> Option<&[u8]> {
        let position = self.entries.iter().position(|e| e.path == path)?;
        let entry = self.entries.remove(position);
        self.entries.push(entry);
        self.entries.last().map(|e| e.data.as_slice())
    }

    /// Removes one entry; returns whether it existed.
    pub fn remove(&mut self, path: &Path) -> bool {
        match self.entries.iter().position(|e| e.path == path) {
            Some(position) => {
                self.entries.remove(position);
                true
            }
            None => false,
        }
    }

    /// Removes every entry, zeroizing the data.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Total number of cached bytes.
    pub fn total_bytes(&self) -> usize {
        self.entries.iter().map(|e| e.data.len()).sum()
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Configured size limit in bytes.
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

/// Everything sensitive that lives only for the current application session.
pub struct Session {
    /// Passwords entered during this session.
    pub passwords: PasswordStore,
    /// Decrypted content kept in memory for fast re-display.
    pub cache: DecryptedCache,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// Creates a session with the default cache policy.
    pub fn new() -> Self {
        Self {
            passwords: PasswordStore::new(),
            cache: DecryptedCache::new(DECRYPTED_CACHE_MAX_BYTES),
        }
    }

    /// The "Forget Password" action.
    pub fn forget_password(&mut self) {
        self.passwords.forget_all();
    }

    /// The "Clear Decrypted Cache" action.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Clears everything; called on normal application exit.
    pub fn shutdown(&mut self) {
        self.forget_password();
        self.clear_cache();
    }

    /// Opens an encrypted file for reading.
    ///
    /// `password` (when given and non-empty) is tried first, then the
    /// passwords retained in this session. A password that works is retained.
    /// The file itself is never modified.
    pub fn unlock(&mut self, path: &Path, password: Option<&str>) -> Result<Decryptor<File>> {
        {
            let mut probe = File::open(path)?;
            format::read_header(&mut probe)?;
        }
        let explicit = password.filter(|p| !p.is_empty());
        let mut attempts: Vec<Zeroizing<String>> = Vec::new();
        if let Some(p) = explicit {
            attempts.push(Zeroizing::new(p.to_string()));
        }
        for candidate in self.passwords.candidates() {
            if Some(candidate) != explicit {
                attempts.push(Zeroizing::new(candidate.to_string()));
            }
        }
        if attempts.is_empty() {
            return Err(Error::PasswordRequired);
        }
        for attempt in &attempts {
            match Decryptor::open(File::open(path)?, attempt) {
                Ok(decryptor) => {
                    self.passwords.remember(attempt);
                    return Ok(decryptor);
                }
                Err(Error::WrongPassword) => continue,
                Err(other) => return Err(other),
            }
        }
        Err(if explicit.is_some() {
            Error::WrongPassword
        } else {
            Error::PasswordRequired
        })
    }
}

#[cfg(test)]
mod tests;
