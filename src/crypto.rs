//! Password-based key derivation, key wrapping and chunked authenticated
//! encryption.
//!
//! * A random 256-bit file key encrypts the content in independent chunks
//!   with XChaCha20-Poly1305. Each chunk nonce combines a random per-file
//!   prefix, the chunk index and a "final chunk" flag, and every chunk
//!   authenticates the whole header as associated data. This detects
//!   reordering, truncation, extension and cross-file splicing, and allows
//!   random access for video seeking.
//! * The file key is wrapped with a key derived from the password by
//!   Argon2id.

use std::io::{self, Cursor, ErrorKind, Read, Seek, SeekFrom, Write};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::Zeroizing;

use crate::constants::{
    CHUNK_SIZE_LOG2, COMPRESSION_DEFLATE, COMPRESSION_MIN_SAVING_PERCENT, COMPRESSION_NONE,
    COMPRESSION_SAMPLE_LEN, FORMAT_VERSION, HEADER_LEN, IO_BLOCK_LEN, KEY_LEN, NONCE_LEN,
    NONCE_PREFIX_LEN, SALT_LEN, TAG_LEN, WRAPPED_KEY_LEN,
};
use crate::error::{Error, Result};
use crate::format::{self, Header, KdfParams};

/// Options controlling encryption.
#[derive(Debug, Clone, Copy)]
pub struct EncryptOptions {
    /// Key-derivation cost parameters written into the header.
    pub kdf: KdfParams,
    /// Compress the content first when a sample shows a clear benefit.
    pub compress: bool,
}

impl Default for EncryptOptions {
    fn default() -> Self {
        Self {
            kdf: KdfParams::default(),
            compress: crate::constants::COMPRESS_ON_ENCRYPT_DEFAULT,
        }
    }
}

/// Summary of a completed encryption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncryptStats {
    /// Number of plaintext bytes read.
    pub plaintext_len: u64,
    /// Number of bytes handed to the cipher (after optional compression).
    pub stored_len: u64,
    /// Compression identifier that was used.
    pub compression: u8,
    /// Total length of the produced encrypted file, header included.
    pub ciphertext_len: u64,
}

fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    OsRng
        .try_fill_bytes(&mut out)
        .map_err(|_| Error::Crypto("system random number generator unavailable"))?;
    Ok(out)
}

fn cipher_for(key: &[u8; KEY_LEN]) -> Result<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Crypto("invalid key length"))
}

/// Derives the key-encryption key from a password with Argon2id.
pub fn derive_kek(
    password: &str,
    salt: &[u8; SALT_LEN],
    params: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    if password.is_empty() {
        return Err(Error::EmptyPassword);
    }
    params.validate()?;
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|e| Error::Kdf(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut out[..])
        .map_err(|e| Error::Kdf(e.to_string()))?;
    Ok(out)
}

/// Encrypts (wraps) a file key under the key-encryption key.
pub fn wrap_file_key(
    kek: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    file_key: &[u8; KEY_LEN],
) -> Result<[u8; WRAPPED_KEY_LEN]> {
    let cipher = cipher_for(kek)?;
    let sealed = cipher
        .encrypt(XNonce::from_slice(nonce), Payload { msg: file_key, aad })
        .map_err(|_| Error::Crypto("key wrapping failed"))?;
    let mut out = [0u8; WRAPPED_KEY_LEN];
    if sealed.len() != WRAPPED_KEY_LEN {
        return Err(Error::Crypto("unexpected wrapped key length"));
    }
    out.copy_from_slice(&sealed);
    Ok(out)
}

/// Decrypts (unwraps) a file key; failure means wrong password or tampering.
pub fn unwrap_file_key(
    kek: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    wrapped: &[u8; WRAPPED_KEY_LEN],
) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    let cipher = cipher_for(kek)?;
    let opened = Zeroizing::new(
        cipher
            .decrypt(XNonce::from_slice(nonce), Payload { msg: wrapped, aad })
            .map_err(|_| Error::WrongPassword)?,
    );
    if opened.len() != KEY_LEN {
        return Err(Error::WrongPassword);
    }
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    key.copy_from_slice(&opened);
    Ok(key)
}

/// Builds the nonce of a content chunk: prefix, big-endian index, final flag.
pub fn chunk_nonce(prefix: &[u8; NONCE_PREFIX_LEN], index: u64, is_last: bool) -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    nonce[..NONCE_PREFIX_LEN].copy_from_slice(prefix);
    nonce[NONCE_PREFIX_LEN..NONCE_LEN - 1].copy_from_slice(&index.to_be_bytes());
    nonce[NONCE_LEN - 1] = u8::from(is_last);
    nonce
}

/// Decides from a leading sample whether DEFLATE compression is worthwhile.
pub fn should_compress(sample: &[u8]) -> bool {
    if sample.is_empty() {
        return false;
    }
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    if encoder.write_all(sample).is_err() {
        return false;
    }
    let Ok(compressed) = encoder.finish() else {
        return false;
    };
    if compressed.len() >= sample.len() {
        return false;
    }
    (sample.len() - compressed.len()) * 100 >= sample.len() * COMPRESSION_MIN_SAVING_PERCENT
}

fn read_up_to<R: Read>(reader: &mut R, limit: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.by_ref().take(limit as u64).read_to_end(&mut out)?;
    Ok(out)
}

fn pump<R: Read, S: Write>(input: &mut R, sink: &mut S) -> io::Result<u64> {
    let mut block = Zeroizing::new(vec![0u8; IO_BLOCK_LEN]);
    let mut total = 0u64;
    loop {
        let n = match input.read(&mut block) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        sink.write_all(&block[..n])?;
        total += n as u64;
    }
    Ok(total)
}

/// Splits a byte stream into chunks and writes each as an authenticated record.
///
/// A chunk is only emitted as non-final once more data follows it, so the last
/// chunk (possibly empty, only when the whole payload is empty) carries the
/// final flag.
struct ChunkSealer<'a, W: Write> {
    writer: &'a mut W,
    cipher: XChaCha20Poly1305,
    aad: [u8; HEADER_LEN],
    nonce_prefix: [u8; NONCE_PREFIX_LEN],
    chunk_size: usize,
    buf: Zeroizing<Vec<u8>>,
    index: u64,
    stored: u64,
    written: u64,
}

impl<'a, W: Write> ChunkSealer<'a, W> {
    fn new(writer: &'a mut W, cipher: XChaCha20Poly1305, header: &Header) -> Self {
        let chunk_size = header.chunk_size();
        Self {
            writer,
            cipher,
            aad: header.chunk_aad(),
            nonce_prefix: header.nonce_prefix,
            chunk_size,
            buf: Zeroizing::new(Vec::with_capacity(chunk_size + IO_BLOCK_LEN)),
            index: 0,
            stored: 0,
            written: 0,
        }
    }

    fn emit(&mut self, last: bool) -> io::Result<()> {
        let take = if last {
            self.buf.len()
        } else {
            self.chunk_size
        };
        let nonce = chunk_nonce(&self.nonce_prefix, self.index, last);
        let sealed = self
            .cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &self.buf[..take],
                    aad: &self.aad,
                },
            )
            .map_err(|_| io::Error::other("authenticated encryption failed"))?;
        self.writer.write_all(&sealed)?;
        self.written += sealed.len() as u64;
        drop(self.buf.drain(..take));
        self.index += 1;
        Ok(())
    }

    /// Emits the final chunk; returns (bytes stored, bytes written).
    fn finish(mut self) -> io::Result<(u64, u64)> {
        self.emit(true)?;
        self.writer.flush()?;
        Ok((self.stored, self.written))
    }
}

impl<W: Write> Write for ChunkSealer<'_, W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        self.stored += data.len() as u64;
        while self.buf.len() > self.chunk_size {
            self.emit(false)?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Encrypts everything readable from `reader` into `writer`.
///
/// The header is written first, so on error the output is incomplete and must
/// be discarded by the caller.
pub fn encrypt_stream<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    password: &str,
    options: &EncryptOptions,
) -> Result<EncryptStats> {
    if password.is_empty() {
        return Err(Error::EmptyPassword);
    }
    options.kdf.validate()?;

    let sample = if options.compress {
        read_up_to(reader, COMPRESSION_SAMPLE_LEN)?
    } else {
        Vec::new()
    };
    let compress = options.compress && should_compress(&sample);
    let mut input = Cursor::new(sample).chain(&mut *reader);

    let file_key = Zeroizing::new(random_array::<KEY_LEN>()?);
    let salt = random_array::<SALT_LEN>()?;
    let wrap_nonce = random_array::<NONCE_LEN>()?;
    let nonce_prefix = random_array::<NONCE_PREFIX_LEN>()?;

    let kek = derive_kek(password, &salt, &options.kdf)?;
    let mut header = Header {
        version: FORMAT_VERSION,
        compression: if compress {
            COMPRESSION_DEFLATE
        } else {
            COMPRESSION_NONE
        },
        chunk_size_log2: CHUNK_SIZE_LOG2,
        kdf: options.kdf,
        salt,
        wrap_nonce,
        wrapped_key: [0u8; WRAPPED_KEY_LEN],
        nonce_prefix,
    };
    header.wrapped_key = wrap_file_key(&kek, &wrap_nonce, &header.key_wrap_aad(), &file_key)?;
    writer.write_all(&header.to_bytes())?;

    let sealer = ChunkSealer::new(&mut *writer, cipher_for(&file_key)?, &header);
    let (plaintext_len, stored_len, written) = if compress {
        let mut encoder = DeflateEncoder::new(sealer, Compression::default());
        let plaintext_len = pump(&mut input, &mut encoder)?;
        let sealer = encoder.finish()?;
        let (stored, written) = sealer.finish()?;
        (plaintext_len, stored, written)
    } else {
        let mut sealer = sealer;
        let plaintext_len = pump(&mut input, &mut sealer)?;
        let (stored, written) = sealer.finish()?;
        (plaintext_len, stored, written)
    };

    Ok(EncryptStats {
        plaintext_len,
        stored_len,
        compression: header.compression,
        ciphertext_len: HEADER_LEN as u64 + written,
    })
}

fn map_read_error(error: io::Error) -> Error {
    match error.kind() {
        ErrorKind::InvalidData | ErrorKind::InvalidInput | ErrorKind::UnexpectedEof => {
            Error::Corrupted
        }
        _ => Error::Io(error),
    }
}

/// Random-access, authenticating reader over the payload of an encrypted file.
///
/// `Read` and `Seek` operate on the stored payload: the plaintext for
/// uncompressed files, or the DEFLATE stream for compressed ones. Use
/// [`Decryptor::read_plaintext`] to obtain decompressed content.
pub struct Decryptor<R: Read + Seek> {
    reader: R,
    header: Header,
    cipher: XChaCha20Poly1305,
    aad: [u8; HEADER_LEN],
    chunk_size: usize,
    payload_len: u64,
    chunk_count: u64,
    pos: u64,
    cached: Option<(u64, Zeroizing<Vec<u8>>)>,
}

impl<R: Read + Seek> Decryptor<R> {
    /// Opens an encrypted stream, verifying the password and file geometry.
    ///
    /// Chunk contents are authenticated lazily as they are read.
    pub fn open(mut reader: R, password: &str) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;
        let header = format::read_header(&mut reader)?;
        let file_len = reader.seek(SeekFrom::End(0))?;
        let kek = derive_kek(password, &header.salt, &header.kdf)?;
        let file_key = unwrap_file_key(
            &kek,
            &header.wrap_nonce,
            &header.key_wrap_aad(),
            &header.wrapped_key,
        )?;
        let chunk_size = header.chunk_size();
        let payload_len = format::payload_len(file_len, chunk_size)?;
        let chunk_count = if payload_len == 0 {
            1
        } else {
            payload_len.div_ceil(chunk_size as u64)
        };
        let cipher = cipher_for(&file_key)?;
        Ok(Self {
            aad: header.chunk_aad(),
            reader,
            header,
            cipher,
            chunk_size,
            payload_len,
            chunk_count,
            pos: 0,
            cached: None,
        })
    }

    /// The parsed header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Length of the stored payload in bytes.
    pub fn payload_len(&self) -> u64 {
        self.payload_len
    }

    /// Whether the payload is a DEFLATE stream rather than raw content.
    pub fn is_compressed(&self) -> bool {
        self.header.compression == COMPRESSION_DEFLATE
    }

    fn load_chunk(&mut self, index: u64) -> Result<()> {
        if matches!(&self.cached, Some((cached, _)) if *cached == index) {
            return Ok(());
        }
        let chunk = self.chunk_size as u64;
        let is_last = index + 1 == self.chunk_count;
        let plain_len = if is_last {
            self.payload_len - (self.chunk_count - 1) * chunk
        } else {
            chunk
        };
        let cipher_len = usize::try_from(plain_len).map_err(|_| Error::Corrupted)? + TAG_LEN;
        let offset = HEADER_LEN as u64 + index * (chunk + TAG_LEN as u64);
        self.reader.seek(SeekFrom::Start(offset))?;
        let mut sealed = vec![0u8; cipher_len];
        self.reader.read_exact(&mut sealed).map_err(|e| {
            if e.kind() == ErrorKind::UnexpectedEof {
                Error::Corrupted
            } else {
                Error::Io(e)
            }
        })?;
        let nonce = chunk_nonce(&self.header.nonce_prefix, index, is_last);
        let plain = self
            .cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &sealed,
                    aad: &self.aad,
                },
            )
            .map_err(|_| Error::Corrupted)?;
        self.cached = Some((index, Zeroizing::new(plain)));
        Ok(())
    }

    fn authenticate_remaining(&mut self) -> Result<()> {
        let start = match &self.cached {
            Some((index, _)) => index + 1,
            None => 0,
        };
        for index in start..self.chunk_count {
            self.load_chunk(index)?;
        }
        Ok(())
    }

    /// Feeds the complete, decompressed plaintext to `sink` in blocks and
    /// returns its length. Every chunk is authenticated before it is used.
    fn drain_plain<F: FnMut(&[u8]) -> Result<()>>(&mut self, mut sink: F) -> Result<u64> {
        let mut total = 0u64;
        if self.is_compressed() {
            self.pos = 0;
            let mut block = Zeroizing::new(vec![0u8; IO_BLOCK_LEN]);
            let mut decoder = DeflateDecoder::new(&mut *self);
            loop {
                let n = decoder.read(&mut block).map_err(map_read_error)?;
                if n == 0 {
                    break;
                }
                sink(&block[..n])?;
                total += n as u64;
            }
            drop(decoder);
            self.authenticate_remaining()?;
        } else {
            for index in 0..self.chunk_count {
                self.load_chunk(index)?;
                if let Some((_, data)) = &self.cached {
                    sink(data.as_slice())?;
                    total += data.len() as u64;
                }
            }
        }
        Ok(total)
    }

    /// Reads and authenticates the whole plaintext into memory, from the
    /// beginning, failing with [`Error::TooLarge`] beyond `max_len` bytes.
    pub fn read_plaintext(&mut self, max_len: usize) -> Result<Zeroizing<Vec<u8>>> {
        let limit = max_len as u64;
        if !self.is_compressed() && self.payload_len > limit {
            return Err(Error::TooLarge { limit });
        }
        let capacity = if self.is_compressed() {
            0
        } else {
            usize::try_from(self.payload_len).unwrap_or(0)
        };
        let mut out = Zeroizing::new(Vec::with_capacity(capacity));
        self.drain_plain(|data| {
            if out.len() as u64 + data.len() as u64 > limit {
                return Err(Error::TooLarge { limit });
            }
            out.extend_from_slice(data);
            Ok(())
        })?;
        Ok(out)
    }
}

impl<R: Read + Seek> Read for Decryptor<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.payload_len == 0 {
            self.load_chunk(0).map_err(io::Error::from)?;
            return Ok(0);
        }
        if out.is_empty() || self.pos >= self.payload_len {
            return Ok(0);
        }
        let chunk = self.chunk_size as u64;
        self.load_chunk(self.pos / chunk).map_err(io::Error::from)?;
        let within = (self.pos % chunk) as usize;
        let data: &[u8] = match &self.cached {
            Some((_, data)) => data.get(within..).unwrap_or(&[]),
            None => &[],
        };
        let n = data.len().min(out.len());
        out[..n].copy_from_slice(&data[..n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for Decryptor<R> {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        let (base, delta) = match target {
            SeekFrom::Start(n) => (0i128, i128::from(n)),
            SeekFrom::Current(d) => (i128::from(self.pos), i128::from(d)),
            SeekFrom::End(d) => (i128::from(self.payload_len), i128::from(d)),
        };
        let new = base + delta;
        if new < 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "seek before the start of the stream",
            ));
        }
        self.pos = u64::try_from(new)
            .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "seek position out of range"))?;
        Ok(self.pos)
    }
}

/// Decrypts a whole stream into `writer`, returning the plaintext length.
///
/// Every chunk is authenticated before its bytes are written. On error the
/// output may be incomplete and must be discarded by the caller.
pub fn decrypt_stream<R: Read + Seek, W: Write>(
    reader: R,
    writer: &mut W,
    password: &str,
) -> Result<u64> {
    let mut decryptor = Decryptor::open(reader, password)?;
    decryptor.drain_plain(|data| writer.write_all(data).map_err(Error::from))
}

#[cfg(test)]
mod tests;
