# Roadmap

The project is paused after the PDF, audio, theater-mode and boss-key work. This
is the order in which to resume. Constants live in `src/constants.rs`; every new
constant needs a test; keep `unsafe` out; update README, USER_GUIDE and (for
format changes) FORMAT.md with every change.

## 0. When resuming, first

1. Run `cargo fmt`, `cargo clippy --all-targets`, `cargo test --lib` and
   `cargo test --lib --no-default-features`; note the real test totals here.
2. Reproduce and bisect KI-2 (Windows flicker). Likely options: revert
   `WindowSequencer` (the previous build only hid decorations on Linux and passed
   the manual checks on both systems), or find what differs in frame timing.
3. Decide what to do about KI-1 (see item 4).

## 1. Change the password of encrypted files

**Key finding:** in format v1 the associated data of every content chunk is the
entire 129-byte header, including the salt, wrap nonce and wrapped key. Changing
the password rewrites exactly those fields, so a header-only rewrite makes every
chunk fail authentication. With v1 a password change means re-encrypting the whole file.

**Recommended direction: format v2.**

- Chunk associated data covers only immutable fields: magic, version, algorithm
  ids, compression id, chunk exponent and nonce prefix. (The key wrap already
  authenticates the KDF parameters and salt; the wrapped key is protected by its
  own tag; the nonce prefix is part of every chunk nonce. Review this reasoning
  before relying on it.)
- A password change rewrites only the key-wrap region of the header: salt, wrap
  nonce, wrapped key and optionally the KDF parameters. The content is untouched.
- Version 2 readers still read v1. A v1 file is migrated once by re-encryption
  through the existing staging procedure (`workflow`).
- The in-place header rewrite must be crash-safe: back up the old header, write
  the new one, re-verify by unwrapping, then remove the backup. Use fault
  injection like `workflow::Hooks` in the tests.
- Tests: tampering with each header field, wrong old password changes nothing,
  v1 files still decrypt, mixed batches, interrupted rewrite. Update FORMAT.md and the
  test `chunk_aad_is_the_whole_serialized_header`.

**Security note to document:** rewrapping keeps the same file key. Anyone who knew
the old password and has an older copy of the file (backup, sync history) can
still unwrap that file's key. A separate **re-key and re-encrypt** action (new file
key) is the only way to cut that off for already-copied files.

UI: single file and batch (one new password per batch), old password from the
session or a prompt, confirm field and lost-password warning, per-file outcomes
(changed, failed and untouched, uncertain with a recovery copy) like encryption.

## 2. Decrypt files to disk

Originally out of scope (the principle is that no plaintext is intentionally
written); now wanted as an explicit user action.

- Confirmation dialog stating that a plaintext copy will be written.
- Destination from `naming::decryption_destination` (exists and is tested but
  unused): restore the original name, never overwrite, ask when the name cannot
  be recovered or exists.
- Decrypt through a hidden staging file and move it into place only after every
  chunk has authenticated (`decrypt_stream` writes earlier chunks before a later
  chunk can fail, so staging plaintext must be removed, ideally overwritten, on failure).
- Ask what happens to the encrypted file afterwards (keep or remove).
- Batch with per-file outcomes, compressed payloads, session or prompted password.
- Update the privacy wording in README, USER_GUIDE and the security limitations.

## 3. Linux theater mode (KI-1)

Options, cheapest first: test on native Linux (Wayland and X11) to learn whether
this is WSLg-only; try forcing X11 under WSLg (needs the X11 libraries); upgrade
eframe to 0.32 or later (Wayland sizing fixes; port the UI code and re-verify
everything on all platforms); report to winit and WSLg with the console and
screenshots; if nothing works, document it as a WSLg limitation and consider
removing the Linux-specific window sequencing (`WindowSequencer`) to cut complexity.

## 4. Quality and robustness

- Run Argon2 on a worker thread when opening encrypted files (KI-9).
- PDF: keep a render cache per document if hayro allows it; tiled rendering for
  huge pages; optional text search; revisit newer hayro releases (0.8 exists; the
  API used here was verified for 0.6 only).
- Audio: verify with real files; playlist or folder auto-advance; better
  audio-only container detection.
- Images: tiled rendering for very large images.
- Publish known-answer test vectors for the file format; arrange an independent
  security review before anyone relies on it.

## 5. Release hygiene

- `THIRD_PARTY_NOTICES.md`: reproduce the PDFium/Foxit font license notice
  (shipped as `LICENSE_FOXIT` in the `hayro-interpret` crate), check `cargo
  license`/`cargo deny` for the full transitive list, and note GStreamer's LGPL.
- CI on Windows, Linux and macOS (build, clippy, tests with and without `video`).
- Re-check the README dependency/license table and GStreamer install steps.
- First macOS run; first run on native Linux.

## Working agreement (what made this project go smoothly)

Local files are the source of truth; changes are delivered as whole files or exact
edits with a verified/assumed/unverified split; CRLF line endings; the Linux
sandbox build used `rustc 1.91` with `--ignore-rust-version` because hayro needs
1.92 (do not trust results for the version check itself).
