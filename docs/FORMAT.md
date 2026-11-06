# VaultView encrypted-file format, versions 1 and 2

This document specifies the file format written and read by VaultView. It is meant to be sufficient for an independent, compatible implementation. The authoritative constants live in `src/constants.rs`.

## 1. Overview

An encrypted file is a fixed 129-byte header followed by an authenticated payload.

- A random 256-bit **file key** encrypts the content.
- The file key is **wrapped** (encrypted) under a **key-encryption key** derived from the password with **Argon2id**.
- Content is split into 64 KiB chunks, each sealed with **XChaCha20-Poly1305**. Chunk nonces encode the chunk position and a "final chunk" flag, and every chunk authenticates the header or its immutable prefix. Reordering, truncating, extending, splicing and modifying are detected, and chunks can be decrypted with random access.
- **Format version 2** (current default) decouples the key-wrap fields from payload chunk authentication, enabling instant, in-place per-file password updates without re-encrypting the file contents.
- **Format version 1** (legacy) authenticates the full serialized header in each chunk. Version 2 readers remain fully backward-compatible with Version 1 files.
- The format is independent of the original media format and stores no file name, size, time or password.

## 2. Conventions

- All multi-byte integers are **little-endian**, except the chunk index inside nonces, which is **big-endian**.
- Offsets are zero-based byte offsets from the start of the file.
- The password is the UTF-8 byte sequence exactly as typed, with **no Unicode normalization**. An empty password is invalid.

## 3. Header (129 bytes)

| Offset | Size | Field | Value |
|---:|---:|---|---|
| 0 | 8 | magic | `56 56 4C 54 89 0D 0A 1A` (`"VVLT"`, `0x89`, CR, LF, `0x1A`) |
| 8 | 2 | format version | `1` (legacy) or `2` (current default) |
| 10 | 1 | KDF id | `1` = Argon2id |
| 11 | 1 | AEAD id | `1` = XChaCha20-Poly1305 |
| 12 | 1 | compression id | `0` = none, `1` = raw DEFLATE |
| 13 | 1 | chunk size exponent | `16` written (chunk size `2^16` = 65536); readers accept 12 to 24 |
| 14 | 4 | KDF memory cost (KiB) | writers use 65536 |
| 18 | 4 | KDF iterations | writers use 3 |
| 22 | 4 | KDF parallelism | writers use 1 |
| 26 | 16 | KDF salt | random, unique per file |
| 42 | 24 | key-wrap nonce | random |
| 66 | 48 | wrapped file key | 32-byte ciphertext + 16-byte tag |
| 114 | 15 | nonce prefix | random, unique per file |

The magic follows the PNG convention (a high-bit byte, CR LF and `0x1A`) so that text-mode transfers that alter line endings corrupt it detectably.

### Reader limits

To prevent crafted headers from forcing unbounded work, readers reject files whose parameters are outside these bounds before running the KDF:

| Parameter | Accepted range |
|---|---|
| memory cost (KiB) | 8 to 1,048,576 and at least 8 × parallelism |
| iterations | 1 to 64 |
| parallelism | 1 to 16 |
| chunk size exponent | 12 to 24 |

## 4. Key derivation

```
KEK = Argon2id(password, salt, m = memory_kib, t = iterations, p = parallelism,
               version = 0x13, output length = 32 bytes)
```

No secret key, associated data or personalization string is used.

## 5. Key wrapping

```
wrap_aad    = header bytes [0, 42)            (magic through salt)
wrapped_key = XChaCha20-Poly1305-Encrypt(key = KEK, nonce = wrap_nonce,
                                          aad = wrap_aad, plaintext = file_key)
```

The result is 48 bytes (32 + 16). Because `wrap_aad` covers the version, algorithm ids, compression id, chunk size exponent, KDF parameters and salt, changing any of them makes unwrapping fail. Changing the wrap nonce or wrapped key also makes unwrapping fail. A failed unwrap is reported as "wrong password or modified key header"; the two cases cannot be distinguished.

## 6. Payload

### 6.1 Stored stream

The **stored stream** is the plaintext if the compression id is `0`, or the plaintext compressed as a single raw DEFLATE stream (RFC 1951, no zlib or gzip wrapper) if the id is `1`. Writers choose compression freely; the reference implementation compresses only if deflating the first 256 KiB saves at least 10%. Compressed payloads can only be decrypted sequentially.

### 6.2 Chunking

The stored stream is split into chunks of at most `chunk_size` bytes. Every chunk except the last holds exactly `chunk_size` bytes. The last chunk holds between 1 and `chunk_size` bytes, except that an **empty stored stream is written as one empty chunk**. A non-final chunk is only emitted once more data is known to follow, so the last chunk is never omitted.

### 6.3 Chunk sealing and Associated Authenticated Data (AAD)

For chunk number `i` (starting at 0):

```
nonce = nonce_prefix (15 bytes) || i as u64 big-endian (8 bytes) || final_flag (1 byte)
        final_flag = 1 for the last chunk, otherwise 0
```

The chunk Associated Authenticated Data (`aad`) differs between format versions:
- **Format version 1**:
  ```
  aad = the complete 129-byte serialized header
  ```
- **Format version 2**:
  ```
  aad = header bytes [0, 14) || nonce_prefix (15 bytes)    (29 bytes total)
  ```
  The 29 bytes authenticate the immutable protocol fields (magic, version, KDF id, AEAD id, compression id, chunk size log2) and the file's random nonce prefix. The key-wrap fields (`salt`, `wrap_nonce`, and `wrapped_key`) are excluded, as they are authenticated independently by the key-wrap AEAD.

Each record is sealed:
```
record = XChaCha20-Poly1305-Encrypt(key = file_key, nonce, aad, plaintext = chunk)
```

Each record is its chunk length plus a 16-byte tag. Records are concatenated directly after the header with no separators.

### 6.4 Deriving the payload length

Given the total file length `L`, header length `H = 129`, tag length `T = 16` and chunk size `C`:

```
body   = L - H            (L < H + T is invalid)
stride = C + T
full   = body div stride
rem    = body mod stride
rem == 0                     -> payload = full * C
rem < T, or (rem == T and full > 0) -> invalid (corrupted)
otherwise                    -> payload = full * C + (rem - T)
```

The case `rem == T` with `full == 0` is the empty payload.

## 7. In-Place Password Updates (Format v2)

In Format version 2, changing the password of an encrypted file does not require touching payload chunks:

1. Unwrap `file_key` using the current password (deriving the current KEK from existing `salt`).
2. Generate a fresh 16-byte `salt` and a fresh 24-byte `wrap_nonce`.
3. Derive the new KEK from the new password and fresh salt.
4. Wrap `file_key` under the new KEK with `key_wrap_aad()`.
5. Write the original header to a temporary crash-recovery file `.<name>.vvhdr.tmp` and flush to disk.
6. Overwrite the first 129 bytes of the encrypted file with the new header and flush to disk.
7. Remove the temporary crash-recovery file.

If an encrypted file is in **Format version 1**, changing its password performs an atomic migration: the file is decrypted chunk-by-chunk and re-encrypted as Format version 2.

## 8. Decoding and error classification

Readers must not present content as valid unless every chunk authenticates. Checks are applied in this order:

1. Fewer than 8 bytes, or magic mismatch: **not encrypted**.
2. Version other than `1` or `2`: **unsupported version** (the number is reported).
3. Fewer than 129 bytes: **invalid header (truncated)**.
4. Unknown KDF id, AEAD id or compression id, or chunk exponent out of range: **invalid header**.
5. KDF parameters out of range: **invalid KDF parameters**.
6. Key unwrap failure: **wrong password or modified key header**.
7. Impossible file length (section 6.4): **corrupted**.
8. Any chunk authentication failure, missing or surplus chunk data, or a failure to inflate the payload: **corrupted**.

Chunk failures cover modified ciphertext or tags, a modified nonce prefix, reordered chunks, chunks spliced from another file (different prefix and header), truncation (including at chunk boundaries, because the true last chunk carries the final flag) and appended data.

Decryption never modifies the encrypted file. Output produced before a failure is detected must be discarded by the caller; the reference implementation authenticates each chunk before releasing its bytes.

## 9. Decryption to Disk

Decryption writes plaintext to a chosen destination file:
1. Destination naming follows default stripping of `.vvenc` or collision resolution (`naming::decryption_destination`).
2. Plaintext is decrypted into a temporary staging file `.<dest>.vvdmp`.
3. If decryption fails (e.g. wrong password or corrupted chunk), the staging file is securely zeroized and deleted immediately.
4. On success, the staging file is synced and atomically renamed to the destination.
5. If requested by the user, the encrypted source file is deleted.

## 10. Security properties and non-goals

- Confidentiality and integrity of content and of the header's cryptographic parameters, given a strong password.
- Resistance to chunk reordering, truncation, extension and splicing.
- Offline password guessing costs a full Argon2id evaluation per guess; strength depends on the password and the chosen cost parameters.
- The file length reveals approximate content size (compression, if used, reveals compressibility only through size).
- The format hides the original file name only if the name is changed; the `.vvenc` extension appended to the original name reveals it.
- There is no password recovery and no key escrow.
- In Format v2, password updates preserve the confidentiality of the file key under the new password and do not expose file key bytes to disk unencrypted.
