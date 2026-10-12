//! Encrypted-file header: serialisation, validation and recognition.
//!
//! The layout is documented in `docs/FORMAT.md`. All multi-byte integers are
//! little-endian.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::constants::{
    AEAD_ID_XCHACHA20_POLY1305, ARGON2_MIN_MEMORY_PER_LANE_KIB, CHUNK_SIZE_LOG2,
    CHUNK_SIZE_LOG2_MAX, CHUNK_SIZE_LOG2_MIN, COMPRESSION_DEFLATE, COMPRESSION_NONE,
    FORMAT_VERSION, HEADER_LEN, KDF_ID_ARGON2ID, KDF_ITERATIONS_DEFAULT, KDF_ITERATIONS_MAX,
    KDF_ITERATIONS_MIN, KDF_MEMORY_KIB_DEFAULT, KDF_MEMORY_KIB_MAX, KDF_MEMORY_KIB_MIN,
    KDF_PARALLELISM_DEFAULT, KDF_PARALLELISM_MAX, KDF_PARALLELISM_MIN, KEY_WRAP_AAD_LEN, MAGIC,
    NONCE_LEN, NONCE_PREFIX_LEN, SALT_LEN, TAG_LEN, WRAPPED_KEY_LEN,
};
use crate::error::{Error, Result};

/// Argon2id cost parameters stored in a file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub memory_kib: u32,
    /// Number of iterations.
    pub iterations: u32,
    /// Degree of parallelism.
    pub parallelism: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            memory_kib: KDF_MEMORY_KIB_DEFAULT,
            iterations: KDF_ITERATIONS_DEFAULT,
            parallelism: KDF_PARALLELISM_DEFAULT,
        }
    }
}

impl KdfParams {
    /// Checks that all parameters lie within the accepted bounds.
    ///
    /// Bounds are enforced when reading files so that a crafted header cannot
    /// force unbounded memory or time consumption.
    pub fn validate(&self) -> Result<()> {
        let in_bounds = (KDF_MEMORY_KIB_MIN..=KDF_MEMORY_KIB_MAX).contains(&self.memory_kib)
            && (KDF_ITERATIONS_MIN..=KDF_ITERATIONS_MAX).contains(&self.iterations)
            && (KDF_PARALLELISM_MIN..=KDF_PARALLELISM_MAX).contains(&self.parallelism)
            && self.memory_kib >= ARGON2_MIN_MEMORY_PER_LANE_KIB.saturating_mul(self.parallelism);
        if in_bounds {
            Ok(())
        } else {
            Err(Error::InvalidKdfParams)
        }
    }
}

/// Parsed encrypted-file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Format version.
    pub version: u16,
    /// Compression identifier of the stored payload.
    pub compression: u8,
    /// Base-2 logarithm of the plaintext chunk size.
    pub chunk_size_log2: u8,
    /// Argon2id parameters used to derive the key-encryption key.
    pub kdf: KdfParams,
    /// Unique Argon2 salt.
    pub salt: [u8; SALT_LEN],
    /// Nonce used to wrap the file key.
    pub wrap_nonce: [u8; NONCE_LEN],
    /// File key encrypted under the password-derived key, with its tag.
    pub wrapped_key: [u8; WRAPPED_KEY_LEN],
    /// Random prefix of every chunk nonce.
    pub nonce_prefix: [u8; NONCE_PREFIX_LEN],
}

struct Put {
    buf: [u8; HEADER_LEN],
    pos: usize,
}

impl Put {
    fn put(&mut self, data: &[u8]) {
        self.buf[self.pos..self.pos + data.len()].copy_from_slice(data);
        self.pos += data.len();
    }
}

struct Take<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Take<'_> {
    fn array<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        out.copy_from_slice(&self.buf[self.pos..self.pos + N]);
        self.pos += N;
        out
    }

    fn u8(&mut self) -> u8 {
        self.array::<1>()[0]
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.array::<4>())
    }
}

impl Header {
    /// Serialises the header into its fixed-size on-disk representation.
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut out = Put { buf: [0u8; HEADER_LEN], pos: 0 };
        out.put(&MAGIC);
        out.put(&self.version.to_le_bytes());
        out.put(&[KDF_ID_ARGON2ID, AEAD_ID_XCHACHA20_POLY1305, self.compression, self.chunk_size_log2]);
        out.put(&self.kdf.memory_kib.to_le_bytes());
        out.put(&self.kdf.iterations.to_le_bytes());
        out.put(&self.kdf.parallelism.to_le_bytes());
        out.put(&self.salt);
        out.put(&self.wrap_nonce);
        out.put(&self.wrapped_key);
        out.put(&self.nonce_prefix);
        out.buf
    }

    /// Parses and validates a header from the start of `bytes`.
    ///
    /// Bytes after the header are ignored. Errors are reported in this order:
    /// missing magic, unsupported version, truncation, unknown identifiers,
    /// out-of-range parameters.
    pub fn from_bytes(bytes: &[u8]) -> Result<Header> {
        if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
            return Err(Error::NotEncrypted);
        }
        if bytes.len() < MAGIC.len() + 2 {
            return Err(Error::InvalidHeader("truncated header"));
        }
        let mut cur = Take { buf: bytes, pos: MAGIC.len() };
        let version = u16::from_le_bytes(cur.array::<2>());
        if version != FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        if bytes.len() < HEADER_LEN {
            return Err(Error::InvalidHeader("truncated header"));
        }
        if cur.u8() != KDF_ID_ARGON2ID {
            return Err(Error::InvalidHeader("unknown key-derivation function"));
        }
        if cur.u8() != AEAD_ID_XCHACHA20_POLY1305 {
            return Err(Error::InvalidHeader("unknown encryption algorithm"));
        }
        let compression = cur.u8();
        if compression != COMPRESSION_NONE && compression != COMPRESSION_DEFLATE {
            return Err(Error::InvalidHeader("unknown compression method"));
        }
        let chunk_size_log2 = cur.u8();
        if !(CHUNK_SIZE_LOG2_MIN..=CHUNK_SIZE_LOG2_MAX).contains(&chunk_size_log2) {
            return Err(Error::InvalidHeader("chunk size out of range"));
        }
        let kdf = KdfParams {
            memory_kib: cur.u32(),
            iterations: cur.u32(),
            parallelism: cur.u32(),
        };
        kdf.validate()?;
        Ok(Header {
            version,
            compression,
            chunk_size_log2,
            kdf,
            salt: cur.array::<SALT_LEN>(),
            wrap_nonce: cur.array::<NONCE_LEN>(),
            wrapped_key: cur.array::<WRAPPED_KEY_LEN>(),
            nonce_prefix: cur.array::<NONCE_PREFIX_LEN>(),
        })
    }

    /// Associated data authenticated by the key wrap: everything up to and
    /// including the salt.
    pub fn key_wrap_aad(&self) -> Vec<u8> {
        self.to_bytes()[..KEY_WRAP_AAD_LEN].to_vec()
    }

    /// Associated data authenticated by every content chunk: the whole header.
    pub fn chunk_aad(&self) -> [u8; HEADER_LEN] {
        self.to_bytes()
    }

    /// Plaintext chunk size in bytes.
    pub fn chunk_size(&self) -> usize {
        1usize
            .checked_shl(u32::from(self.chunk_size_log2))
            .unwrap_or(1usize << CHUNK_SIZE_LOG2)
    }
}

/// Returns `true` when `prefix` starts with the encrypted-file magic value.
pub fn is_encrypted_prefix(prefix: &[u8]) -> bool {
    prefix.len() >= MAGIC.len() && prefix[..MAGIC.len()] == MAGIC
}

/// Returns `true` when the file at `path` starts with the magic value.
///
/// Recognition never depends on the file name.
pub fn is_encrypted_file(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut buf = [0u8; MAGIC.len()];
    file.read_exact(&mut buf).is_ok() && is_encrypted_prefix(&buf)
}

/// Reads and parses a header from the current position of `reader`.
pub fn read_header<R: Read>(reader: &mut R) -> Result<Header> {
    let mut buf = [0u8; HEADER_LEN];
    let mut filled = 0;
    while filled < HEADER_LEN {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(Error::Io(e)),
        }
    }
    Header::from_bytes(&buf[..filled])
}

/// Computes the plaintext payload length from the total file length.
///
/// The payload consists of chunks of `chunk_size` bytes plus a tag each; the
/// final chunk may be shorter. An empty payload is stored as a single empty
/// chunk. Lengths that no writer could have produced are reported as
/// [`Error::Corrupted`].
pub fn payload_len(file_len: u64, chunk_size: usize) -> Result<u64> {
    let header = HEADER_LEN as u64;
    let tag = TAG_LEN as u64;
    if file_len < header + tag {
        return Err(Error::Corrupted);
    }
    let body = file_len - header;
    let chunk = chunk_size as u64;
    let stride = chunk + tag;
    let full = body / stride;
    let rem = body % stride;
    if rem == 0 {
        return Ok(full * chunk);
    }
    if rem < tag || (rem == tag && full > 0) {
        return Err(Error::Corrupted);
    }
    Ok(full * chunk + (rem - tag))
}

#[cfg(test)]
mod tests;