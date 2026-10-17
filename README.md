## Project status

Paused. See [docs/KNOWN_ISSUES.md](docs/KNOWN_ISSUES.md) for known bugs and unverified areas, and [docs/ROADMAP.md](docs/ROADMAP.md) for what comes next.

# VaultView

A cross-platform desktop media viewer with integrated, password-based file encryption, favorite folders, and privacy-conscious file deletion. It is written in Rust and licensed under the MIT license.

VaultView lets you browse local folders, view images and play videos, and encrypt files **in place** with authenticated encryption. Encrypted media is decrypted only in memory while you view it. The application never intentionally writes decrypted content to disk.

> **Read this first.** If you lose the password of an encrypted file, the file is permanently unrecoverable. There is no recovery mechanism, no account and no remote service. Deep deletion and in-place encryption **cannot guarantee** that old copies of data are physically erased from the storage device. See [Security model and limitations](#security-model-and-limitations).

## Features

- **File browser** with drive/home shortcuts, an editable path bar, natural sorting (`img2` before `img10`), optional hidden files, and multi-select (check boxes, Shift to extend a range).
- **Content-based file identification**: files are classified by their leading bytes, with the extension used only as a fallback.
- **Images**: JPEG, PNG, GIF, WebP, BMP, TIFF (first page), animated GIF and animated WebP, EXIF orientation for JPEG, zoom in/out, 100%, fit to window, fit image (never enlarges), reset, pan by dragging or `W A S D`, zoom at the pointer with the mouse wheel. AVIF is available as an opt-in build feature (see below).
- **Video** through GStreamer: play/pause, seek bar, volume, mute, full screen, audio-track and subtitle selection where the container provides them, rotation metadata where the `videoflip` plugin is installed. Codec support is whatever your GStreamer installation provides.
- **PDF** (pure-Rust renderer, no native library to install): page navigation (buttons, page box, Page Up/Page Down/Home/End), zoom in/out, 100%, fit to window, reset, pan, mouse-wheel zoom at the pointer. Rendering runs on a background thread; damaged files, PDFs protected by their own password and individual pages that cannot be rendered produce a message instead of a crash, and unsupported content on a page is reported while the rest is shown.
- **Audio** through the same GStreamer pipeline as video: play/pause, seek bar, volume, mute, and whatever tags (title, artist, album, genre, codec, bit rate) and embedded cover art the file carries. Supported formats are those of your GStreamer installation (normally MP3, AAC/M4A, FLAC, Ogg Vorbis, Opus and WAV). Audio needs the default `video` Cargo feature.
- **Theater mode** (`F`, `F11`, the *Theater* button): borderless full screen on the window's display showing only the viewing pane. Controls appear in an overlay when the mouse moves and fade after a few seconds, with an Exit button; the cursor hides with them.
- **Boss key** (`Esc`): runs *Lock all* (closes the media, forgets passwords, clears the decrypted cache), closes open dialogs, leaves theater mode and minimizes the window. It works only while the application window has the keyboard focus; there is no system-wide hotkey.
- **Navigation**: Left/Right arrow keys and on-screen buttons move through the images, videos, audio files, PDFs and encrypted files of the current folder in a consistent order. A failed open (for example a wrong password) never advances to another file.
- **Favorites**: mark folders as favorites; they persist across sessions, store only folder paths, and are flagged when missing, not a folder, or inaccessible.
- **Encryption**: one password per batch, an independent random key per file, XChaCha20-Poly1305 authenticated encryption in 64 KiB chunks, Argon2id key derivation, optional compression when it clearly helps. Each file is independently decryptable.
- **In-place replacement** with verification and per-file outcomes: *encrypted*, *failed (file untouched)* or *uncertain (recovery copy kept)*.
- **Encrypted media viewing** without plaintext files: images are decrypted into memory; uncompressed encrypted video is streamed to the player in bounded blocks with random access for seeking.
- **Privacy controls**: Forget Password, Clear Decrypted Cache, Close Media, and Lock All (all three). The decrypted cache and session passwords are cleared on normal exit.
- **Deletion**: normal deletion (operating-system file deletion) and deep deletion (three random overwrite passes, then deletion), both with explicit confirmation.

## Supported platforms

| Platform | Status |
|---|---|
| Windows 10 | Run by the author; unit tests pass. Theater-mode flicker reported again (KI-2). |
| Linux | Run on Ubuntu 22.04 under WSL2/WSLg; theater mode leaves a window frame when the window was maximized (KI-1). Native Linux not tried. |
| macOS | Never tried. |

The encrypted-file format is portable: a file encrypted on one platform decrypts on any other using the same password and a compatible version.

## Prerequisites

1. **Rust** (stable, 1.92 or newer). The PDF renderer's dependencies require 1.92. Install from <https://rustup.rs>. `rust-toolchain.toml` selects the stable channel with `rustfmt` and `clippy`.
2. **GStreamer 1.x development files and plugins** (only for video playback, the default build). The `video` feature can be switched off, see below.

### Installing GStreamer

These instructions are from the GStreamer documentation as I know it and have not been verified for this project. Consult <https://gstreamer.freedesktop.org/documentation/installing/> if something differs.

**Debian/Ubuntu**

```bash
sudo apt install pkg-config libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly gstreamer1.0-libav
```

The GUI toolkit may additionally need the usual windowing and OpenGL libraries (for example `libxkbcommon-x11-0`, `libgl1`, `libwayland-client0`).

**macOS (Homebrew)**

```bash
brew install pkg-config gstreamer
```

If the official framework installer is used instead, set
`PKG_CONFIG_PATH=/Library/Frameworks/GStreamer.framework/Versions/1.0/lib/pkgconfig`.

**Windows (MSVC)**

1. Install the MSVC Rust toolchain (`stable-x86_64-pc-windows-msvc`) and the Visual Studio Build Tools.
2. Install **both** the *runtime* and *development* MSVC 64-bit GStreamer installers (choose "Complete" installation).
3. Make sure `GSTREAMER_1_0_ROOT_MSVC_X86_64` is set (the installer normally does this), add `%GSTREAMER_1_0_ROOT_MSVC_X86_64%\bin` to `PATH`, and set `PKG_CONFIG_PATH` to `%GSTREAMER_1_0_ROOT_MSVC_X86_64%\lib\pkgconfig`.
4. Install `pkg-config` (for example `pkgconf` through Scoop or Chocolatey).

## Project structure

```
vaultview/
├── Cargo.toml                 manifest, features, lint policy
├── rust-toolchain.toml
├── LICENSE
├── README.md
├── docs/
│   ├── FORMAT.md              encrypted-file format specification
│   └── USER_GUIDE.md          end-user guide
└── src/
    ├── main.rs                starts the GUI
    ├── lib.rs                 library root (all logic and unit tests live in the library)
    ├── constants.rs           every application-level constant, in one place
    ├── error.rs               shared error type
    ├── format.rs              encrypted-file header: encode, decode, recognition
    ├── crypto.rs              Argon2id, key wrapping, chunked AEAD, decrypting reader
    ├── naming.rs              encrypted-extension and filename handling
    ├── workflow.rs            in-place batch encryption and per-file outcomes
    ├── shred.rs               normal deletion and deep deletion
    ├── session.rs             session passwords and the in-memory decrypted cache
    ├── browser.rs             listing, content sniffing, ordering, navigation, selection
    ├── favorites.rs           persistent favorite folders
    ├── media.rs               image decoding, animation, EXIF orientation, scaling
    ├── viewer.rs              zoom/pan/fit calculations
    ├── audio.rs               audio tag/cover-art data and formatting (pure logic)
    ├── pdf.rs                 PDF loading and rendering on a worker thread (hayro)
    ├── theater.rs             overlay timing and the boss-key sequence (pure logic)
    ├── video.rs               GStreamer playback of video and audio, incl. encrypted streams (feature video)
    ├── app.rs                 egui user interface
    └── <module>/tests.rs      unit tests, one directory per module
```

Application-wide constants (security parameters, limits, extensions, defaults) are defined only in `src/constants.rs`. Changes to security-sensitive constants must be deliberate and must stay consistent with `docs/FORMAT.md`.

## Building and running

```bash
cargo build --release          # default build, includes video (needs GStreamer)
cargo run --release
```

Without video support (no GStreamer needed; videos then report "no video support"):

```bash
cargo build --release --no-default-features
cargo run --release --no-default-features
```

Optional AVIF image decoding (requires the native `dav1d` library and its build tools; not verified for this project):

```bash
cargo build --release --features avif
```

On first use the application keeps its favorites in the per-user configuration directory:
`%APPDATA%\vaultview\favorites.json` (Windows), `~/.config/vaultview/favorites.json` (Linux), `~/Library/Application Support/vaultview/favorites.json` (macOS).

## Testing

```bash
cargo test --lib --no-default-features     # unit tests, no GStreamer required
cargo test --lib                           # unit tests with the video feature compiled
cargo clippy --all-targets
```

The unit tests cover the encrypted-file format, key derivation and key wrapping, encryption and decryption (including wrong passwords, tampering, truncation, extension, reordering and splicing), filename handling, in-place encryption with injected failures, deep deletion, session and cache behavior, navigation and selection, favorites persistence, image decoding and orientation, and zoom/pan math. They use only generated data and obviously fake passwords.

The tests for `video.rs` (tag extraction, in-memory source) need the GStreamer libraries to be installed because they create GStreamer tag lists, so they run only in builds with the default `video` feature. The PDF tests render small generated PDFs with the real renderer.


## Security model and limitations

- **Cryptography.** XChaCha20-Poly1305 with a random 256-bit key per file; the key is wrapped under a key derived from the batch password with Argon2id (default 64 MiB, 3 iterations, 1 lane). See `docs/FORMAT.md`. The format and code have not had an independent security review.
- **Password loss.** There is no recovery. Choose a strong password and store it safely. Passwords are used exactly as typed (UTF-8, no normalization).
- **No intentional plaintext on disk.** Decrypted content is held in memory only and zeroized when released, on a best-effort basis. The application cannot control operating-system paging, swap, hibernation, crash dumps, graphics drivers, hardware video decoders, GStreamer's internal buffers, GPU textures, or text-field history of the GUI toolkit, so these may expose decrypted data. The application cannot claim otherwise.
- **In-place encryption is not secure erasure.** The original file's bytes are overwritten with the ciphertext, but journaling filesystems, SSD wear levelling, copy-on-write storage, snapshots and backups may retain earlier plaintext.
- **Deep deletion** overwrites logical file contents three times with random data and then asks the operating system to delete the file. It does not guarantee physical erasure on SSDs, wear-levelled media, journaling or copy-on-write filesystems, snapshots or backups, and does not remove earlier plaintext copies of an encrypted file.
- **Normal deletion** is the operating system's standard file deletion, not the recycle bin or trash.
- **Staging file.** During encryption a hidden sibling file `.<name>.vvtmp` holds a verified ciphertext copy. It is removed on success. If a failure leaves the result uncertain, it is kept on purpose and named in the report; it contains only ciphertext.
- **Memory and time.** Opening an encrypted file runs Argon2 (about 64 MiB) on the interface thread. Encrypting a file derives the key twice (once to encrypt, once to verify the result).
- **PDF parser memory.** Encrypted PDFs are decrypted into memory and handed to the PDF library, which keeps its own copy of the bytes. That copy cannot be zeroized by this application.
- **Boss key.** Esc locks and minimizes only while the application window has focus. Operating systems may keep a snapshot of a window for thumbnails, previews or animations; the application clears the viewer and waits a couple of frames before minimizing to reduce this, but cannot guarantee what the operating system retains. Typed text may also remain in the GUI toolkit's internal text-edit history. A background encryption or deletion started before the boss key keeps running, and its result dialog appears when the window is restored.

## Other known limitations

- Images larger than 8192 pixels on a side are downscaled for display; there is no tiled rendering. Images and animations are limited to 200 million pixels in total, and decrypted or decompressed image data to 256 MiB.
- An encrypted video that was stored compressed cannot be streamed; it is decrypted fully into memory (same 256 MiB limit). Videos encrypted by this version are normally stored uncompressed because compression is only chosen when a sample compresses well.
- Video seeking snaps to nearby key frames. Support for codecs, subtitles, HDR and rotation depends on the installed GStreamer plugins.
- Multi-page TIFF files show only the first page.
- AVIF requires the optional `avif` build feature.
- Hidden-file detection uses the dot prefix and, on Windows, the hidden attribute.
- No thumbnails or previews are generated (deliberately, to avoid persisting content).
- The application does not edit media and is not a general file manager.
- Decrypting to a plain file is not provided: encrypted files can be viewed (decrypted in memory only) but not written back to disk. This is a design decision, not a defect.
- PDF: PDFs protected by their own password are not opened; the renderer is a young pure-Rust one and may skip or misrender some content (JPEG 2000 images, some fonts and scripts, complex transparency). Pages are rendered whole: at very high zoom a large page is rendered at reduced resolution (limit 24 million pixels and 8192 pixels per side) and appears soft. "100%" shows a page at its nominal size of 72 points per inch. There is no text search, selection, links, annotations or form filling. Files over 256 MiB are refused.
- Audio: no playlists, gapless playback, visualizations or equalizer. Cover art is the first embedded image only. An audio-only MKV or MP4 may first be listed as video; the viewer switches to the audio layout once the file has been analysed.
- Theater mode: on macOS the full-screen transition is native and takes a moment; the boss key waits for it to finish before minimizing. Esc no longer leaves full screen (it is the boss key); leave with F, F11 or the overlay's Exit button.
- Esc is always the boss key on the main window, including while a text box or dialog has focus, so pressing it to cancel an edit also locks everything.

## Dependencies and licenses

VaultView is licensed under the MIT license (see `LICENSE`). Direct dependencies and their licenses as I understand them; verify with `cargo license` or `cargo deny` before redistributing binaries:

| Crate | Purpose | License |
|---|---|---|
| eframe / egui | GUI | MIT OR Apache-2.0 |
| image | image decoding | MIT OR Apache-2.0 |
| infer | content detection | MIT |
| chacha20poly1305 | authenticated encryption | Apache-2.0 OR MIT |
| argon2 | password-based key derivation | MIT OR Apache-2.0 |
| rand | system randomness | MIT OR Apache-2.0 |
| zeroize | clearing secrets from memory | Apache-2.0 OR MIT |
| flate2 (miniz_oxide backend) | DEFLATE compression | MIT OR Apache-2.0 |
| serde, serde_json | favorites storage | MIT OR Apache-2.0 |
| thiserror | error types | MIT OR Apache-2.0 |
| dirs | configuration directory | MIT OR Apache-2.0 |
| gstreamer, gstreamer-app, gstreamer-video (optional) | Rust bindings for video playback | MIT OR Apache-2.0 |
| tempfile (tests only) | temporary directories | MIT OR Apache-2.0 |
| hayro (with its hayro-* and vello_cpu dependencies) | PDF rendering | Apache-2.0 OR MIT (as declared by the hayro crates) |

**GStreamer** itself is licensed under the LGPL-2.1-or-later and is linked dynamically; it is installed separately and is not bundled. Individual GStreamer plugins (for example `gst-libav`, which wraps FFmpeg, and the "bad" and "ugly" sets) carry their own licenses and, for some codecs, patent considerations. If you redistribute GStreamer or its plugins with the application, you are responsible for complying with their licenses and notices.

**Bundled PDF fonts.** The `hayro` dependency is built with its `embed-fonts` feature, which compiles the 14 standard PDF fonts (the "Foxit" fonts) into the executable so that text in PDFs that do not embed those fonts can be drawn. The crate ships these fonts with the BSD-3-Clause-style license of the PDFium project (file `LICENSE_FOXIT` in the `hayro-interpret` crate), which requires that binary redistributions reproduce its copyright notice, conditions and disclaimer in the accompanying documentation. It also ships ICC colour profiles under CC0. If you distribute binaries, include that notice (for example in a `THIRD_PARTY_NOTICES` file) and check `cargo license` for the full transitive list.

## Contributing

Run `cargo fmt`, `cargo clippy --all-targets` and `cargo test --lib --no-default-features` before submitting changes. Keep new constants in `src/constants.rs`, avoid `unsafe` (the crate forbids it), and add unit tests for security-sensitive behavior.