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

## 1. Change the password of encrypted files [COMPLETED]

**Completed:** Format v2 is implemented as default:
- Chunk associated data in Format v2 covers immutable fields: `[0..14)` + `nonce_prefix` (29 bytes).
- A password change rewrites only the key-wrap region of the header (salt, wrap nonce, wrapped key).
- In-place header rewrites are crash-safe using temporary backup files (`.<name>.vvhdr.tmp`).
- Seamless backward-compatibility: v2 readers read v1 files. Password change on v1 files migrates them to v2.
- Unit tests verify in-place header rewrapping, tampering detection, and v1-to-v2 migration.

## 2. Decrypt files to disk [COMPLETED]

**Completed:** Explicit user action to decrypt encrypted files back to disk:
- Destination path resolution with conflict handling (`naming::decryption_destination`).
- Staging decryption via `.<name>.vvdmp` with automatic zeroization and shredding on failure.
- Optional removal of encrypted source file after successful decryption.
- Batch and single-file workflow with per-file status reports.

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
