# VaultView

A cross-platform desktop media viewer with integrated, password-based file encryption, favorite folders, and privacy-conscious file deletion. It is written in Rust and licensed under the MIT license.

VaultView lets you browse local folders, view images and play videos, and encrypt files **in place** with authenticated encryption. Encrypted media is decrypted only in memory while you view it. The application never intentionally writes decrypted content to disk.

> **Read this first.** If you lose the password of an encrypted file, the file is permanently unrecoverable. There is no recovery mechanism, no account and no remote service. Deep deletion and in-place encryption **cannot guarantee** that old copies of data are physically erased from the storage device. See [Security model and limitations](#security-model-and-limitations).

## Features

- **File browser** with drive/home shortcuts, an editable path bar, natural sorting (`img2` before `img10`), optional hidden files, and multi-select (check boxes, Shift to extend a range).
- **Content-based file identification**: files are classified by their leading bytes, with the extension used only as a fallback.
- **Images**: JPEG, PNG, GIF, WebP, BMP, TIFF (first page), animated GIF and animated WebP, EXIF orientation for JPEG, zoom in/out, 100%, fit to window, fit image (never enlarges), reset, pan by dragging or `W A S D`, zoom at the pointer with the mouse wheel. AVIF is available as an opt-in build feature (see below).
- **Video** through GStreamer: play/pause, seek bar, volume, mute, full screen, audio-track and subtitle selection where the container provides them, rotation metadata where the `videoflip` plugin is installed. Codec support is whatever your GStreamer installation provides.
- **Navigation**: Left/Right arrow keys and on-screen buttons move through the viewable files of the current folder in a consistent order. A failed open (for example a wrong password) never advances to another file.
- **Favorites**: mark folders as favorites; they persist across sessions, store only folder paths, and are flagged when missing, not a folder, or inaccessible.
- **Encryption**: one password per batch, an independent random key per file, XChaCha20-Poly1305 authenticated encryption in 64 KiB chunks, Argon2id key derivation, optional compression when it clearly helps. Each file is independently decryptable.
- **In-place replacement** with verification and per-file outcomes: *encrypted*, *failed (file untouched)* or *uncertain (recovery copy kept)*.
- **Encrypted media viewing** without plaintext files: images are decrypted into memory; uncompressed encrypted video is streamed to the player in bounded blocks with random access for seeking.
- **Privacy controls**: Forget Password, Clear Decrypted Cache, Close Media, and Lock All (all three). The decrypted cache and session passwords are cleared on normal exit.
- **Deletion**: normal deletion (operating-system file deletion) and deep deletion (three random overwrite passes, then deletion), both with explicit confirmation.

## Supported platforms

| Platform | Status |
|---|---|
| Windows (x86-64, MSVC toolchain) | Builds with and without video; unit tests and clippy pass. GUI and video playback not yet verified at run time. |
| Linux | Targeted, not yet verified. |
| macOS | Targeted, not yet verified. |

The encrypted-file format is portable: a file encrypted on one platform decrypts on any other using the same password and a compatible version.

## Prerequisites

1. **Rust** (stable, 1.85 or newer). Install from <https://rustup.rs>. `rust-toolchain.toml` selects the stable channel with `rustfmt` and `clippy`.
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
    ├── video.rs               GStreamer playback, incl. encrypted streams (feature `video`)
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

Not covered by unit tests: `video.rs` (needs a GStreamer runtime) and `app.rs` (needs a GUI). These require manual or separate integration and platform testing. Unix-only tests (symlink refusal, read-only file handling) run only on Linux and macOS.

## Security model and limitations

- **Cryptography.** XChaCha20-Poly1305 with a random 256-bit key per file; the key is wrapped under a key derived from the batch password with Argon2id (default 64 MiB, 3 iterations, 1 lane). See `docs/FORMAT.md`. The format and code have not had an independent security review.
- **Password loss.** There is no recovery. Choose a strong password and store it safely. Passwords are used exactly as typed (UTF-8, no normalization).
- **No intentional plaintext on disk.** Decrypted content is held in memory only and zeroized when released, on a best-effort basis. The application cannot control operating-system paging, swap, hibernation, crash dumps, graphics drivers, hardware video decoders, GStreamer's internal buffers, GPU textures, or text-field history of the GUI toolkit, so these may expose decrypted data. The application cannot claim otherwise.
- **In-place encryption is not secure erasure.** The original file's bytes are overwritten with the ciphertext, but journaling filesystems, SSD wear levelling, copy-on-write storage, snapshots and backups may retain earlier plaintext.
- **Deep deletion** overwrites logical file contents three times with random data and then asks the operating system to delete the file. It does not guarantee physical erasure on SSDs, wear-levelled media, journaling or copy-on-write filesystems, snapshots or backups, and does not remove earlier plaintext copies of an encrypted file.
- **Normal deletion** is the operating system's standard file deletion, not the recycle bin or trash.
- **Staging file.** During encryption a hidden sibling file `.<name>.vvtmp` holds a verified ciphertext copy. It is removed on success. If a failure leaves the result uncertain, it is kept on purpose and named in the report; it contains only ciphertext.
- **Memory and time.** Opening an encrypted file runs Argon2 (about 64 MiB) on the interface thread. Encrypting a file derives the key twice (once to encrypt, once to verify the result).

## Other known limitations

- Images larger than 8192 pixels on a side are downscaled for display; there is no tiled rendering. Images and animations are limited to 200 million pixels in total, and decrypted or decompressed image data to 256 MiB.
- An encrypted video that was stored compressed cannot be streamed; it is decrypted fully into memory (same 256 MiB limit). Videos encrypted by this version are normally stored uncompressed because compression is only chosen when a sample compresses well.
- Video seeking snaps to nearby key frames. Support for codecs, subtitles, HDR and rotation depends on the installed GStreamer plugins.
- Multi-page TIFF files show only the first page.
- AVIF requires the optional `avif` build feature.
- Hidden-file detection uses the dot prefix and, on Windows, the hidden attribute.
- No thumbnails or previews are generated (deliberately, to avoid persisting content).
- The application does not edit media and is not a general file manager.
- There is no "decrypt to file" function yet: encrypted files can be viewed but not written back to disk as plain files.

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

**GStreamer** itself is licensed under the LGPL-2.1-or-later and is linked dynamically; it is installed separately and is not bundled. Individual GStreamer plugins (for example `gst-libav`, which wraps FFmpeg, and the "bad" and "ugly" sets) carry their own licenses and, for some codecs, patent considerations. If you redistribute GStreamer or its plugins with the application, you are responsible for complying with their licenses and notices.

## Contributing

Run `cargo fmt`, `cargo clippy --all-targets` and `cargo test --lib --no-default-features` before submitting changes. Keep new constants in `src/constants.rs`, avoid `unsafe` (the crate forbids it), and add unit tests for security-sensitive behavior.