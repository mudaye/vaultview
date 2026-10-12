//! Single, central home of every application-level constant.
//!
//! Security-sensitive values (format identifiers, key-derivation bounds, chunk
//! geometry) must only be changed deliberately and together with
//! `docs/FORMAT.md`. Run-time user choices are represented by `*_DEFAULT`
//! values; everything else is a fixed compile-time policy.

// ---------------------------------------------------------------------------
// Identity and file naming
// ---------------------------------------------------------------------------

/// Application name, used for configuration directories and window titles.
pub const APP_NAME: &str = "vaultview";
/// Extension appended to encrypted files (without the leading dot).
pub const ENCRYPTED_EXTENSION: &str = "vvenc";
/// Suffix of the sibling staging file used during in-place encryption.
pub const TEMP_SUFFIX: &str = "vvtmp";
/// Extension suggested when the original name of an encrypted file is unknown.
pub const DECRYPTED_FALLBACK_EXTENSION: &str = "decrypted";

// ---------------------------------------------------------------------------
// Encrypted-file format (see docs/FORMAT.md)
// ---------------------------------------------------------------------------

/// Magic value identifying an encrypted file.
pub const MAGIC: [u8; 8] = *b"VVLT\x89\r\n\x1a";
/// Current encrypted-file format version.
pub const FORMAT_VERSION: u16 = 1;
/// Identifier of Argon2id in the header.
pub const KDF_ID_ARGON2ID: u8 = 1;
/// Identifier of XChaCha20-Poly1305 in the header.
pub const AEAD_ID_XCHACHA20_POLY1305: u8 = 1;
/// Header compression identifier: content stored as-is.
pub const COMPRESSION_NONE: u8 = 0;
/// Header compression identifier: content stored as raw DEFLATE.
pub const COMPRESSION_DEFLATE: u8 = 1;

/// Base-2 logarithm of the plaintext chunk size written by this version.
pub const CHUNK_SIZE_LOG2: u8 = 16;
/// Smallest chunk-size exponent accepted when reading.
pub const CHUNK_SIZE_LOG2_MIN: u8 = 12;
/// Largest chunk-size exponent accepted when reading.
pub const CHUNK_SIZE_LOG2_MAX: u8 = 24;
/// Plaintext chunk size in bytes (64 KiB).
pub const CHUNK_SIZE: usize = 1usize << CHUNK_SIZE_LOG2;

/// Length of a symmetric key in bytes.
pub const KEY_LEN: usize = 32;
/// Length of the Argon2 salt in bytes.
pub const SALT_LEN: usize = 16;
/// Length of an XChaCha20-Poly1305 nonce in bytes.
pub const NONCE_LEN: usize = 24;
/// Length of a Poly1305 authentication tag in bytes.
pub const TAG_LEN: usize = 16;
/// Length of the random per-file nonce prefix (nonce minus counter and flag).
pub const NONCE_PREFIX_LEN: usize = 15;
/// Length of the wrapped file key (key plus tag).
pub const WRAPPED_KEY_LEN: usize = KEY_LEN + TAG_LEN;
/// Total header length in bytes.
pub const HEADER_LEN: usize =
    MAGIC.len() + 2 + 4 + 12 + SALT_LEN + NONCE_LEN + WRAPPED_KEY_LEN + NONCE_PREFIX_LEN;
/// Number of leading header bytes authenticated by the key wrap (through the salt).
pub const KEY_WRAP_AAD_LEN: usize = MAGIC.len() + 2 + 4 + 12 + SALT_LEN;

// ---------------------------------------------------------------------------
// Key derivation (Argon2id)
// ---------------------------------------------------------------------------

/// Default Argon2id memory cost in KiB (64 MiB).
pub const KDF_MEMORY_KIB_DEFAULT: u32 = 65_536;
/// Smallest memory cost accepted when reading a header.
pub const KDF_MEMORY_KIB_MIN: u32 = 8;
/// Largest memory cost accepted when reading a header (1 GiB).
pub const KDF_MEMORY_KIB_MAX: u32 = 1_048_576;
/// Default Argon2id iteration count.
pub const KDF_ITERATIONS_DEFAULT: u32 = 3;
/// Smallest iteration count accepted when reading a header.
pub const KDF_ITERATIONS_MIN: u32 = 1;
/// Largest iteration count accepted when reading a header.
pub const KDF_ITERATIONS_MAX: u32 = 64;
/// Default Argon2id parallelism.
pub const KDF_PARALLELISM_DEFAULT: u32 = 1;
/// Smallest parallelism accepted when reading a header.
pub const KDF_PARALLELISM_MIN: u32 = 1;
/// Largest parallelism accepted when reading a header.
pub const KDF_PARALLELISM_MAX: u32 = 16;
/// Argon2 requires at least this much memory (KiB) per lane.
pub const ARGON2_MIN_MEMORY_PER_LANE_KIB: u32 = 8;

// ---------------------------------------------------------------------------
// Compression and I/O
// ---------------------------------------------------------------------------

/// Number of leading bytes sampled to decide whether compression is worthwhile.
pub const COMPRESSION_SAMPLE_LEN: usize = 256 * 1024;
/// Minimum saving (percent) the sample must show for compression to be used.
pub const COMPRESSION_MIN_SAVING_PERCENT: usize = 10;
/// Block size used when streaming data between readers and writers.
pub const IO_BLOCK_LEN: usize = 64 * 1024;
/// Default for the user option "compress before encrypting when beneficial".
pub const COMPRESS_ON_ENCRYPT_DEFAULT: bool = true;

// ---------------------------------------------------------------------------
// Passwords, session and cache policy
// ---------------------------------------------------------------------------

/// Minimum number of characters for a new encryption password.
pub const MIN_PASSWORD_CHARS: usize = 8;
/// Maximum number of distinct passwords retained during one session.
pub const MAX_SESSION_PASSWORDS: usize = 8;
/// Maximum total size of the in-memory decrypted-content cache.
pub const DECRYPTED_CACHE_MAX_BYTES: usize = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/// Largest encrypted image (decompressed plaintext) that will be decoded.
pub const MAX_DECRYPTED_IMAGE_BYTES: usize = 256 * 1024 * 1024;
/// Largest number of pixels (per image, or summed over animation frames).
pub const MAX_IMAGE_PIXELS: u64 = 200_000_000;
/// Largest texture side length uploaded to the GPU; larger images are downscaled.
pub const MAX_TEXTURE_SIDE: u32 = 8192;
/// Animation frame delays below this value (ms) are replaced by the default.
pub const FRAME_DELAY_MIN_MS: u64 = 20;
/// Delay (ms) used for animation frames with missing or tiny delays.
pub const FRAME_DELAY_DEFAULT_MS: u64 = 100;
/// Smallest zoom factor.
pub const ZOOM_MIN: f32 = 0.05;
/// Largest zoom factor.
pub const ZOOM_MAX: f32 = 32.0;
/// Multiplicative zoom step for the zoom in/out commands.
pub const ZOOM_STEP_FACTOR: f32 = 1.25;
/// Keyboard pan step in screen pixels.
pub const PAN_KEY_STEP_PX: f32 = 80.0;

// ---------------------------------------------------------------------------
// Deletion
// ---------------------------------------------------------------------------

/// Number of overwrite passes performed by deep deletion.
pub const DEEP_DELETE_PASSES: u32 = 3;
/// Size of the random buffer used while overwriting files.
pub const SHRED_BUFFER_LEN: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// Browsing and favorites
// ---------------------------------------------------------------------------

/// Number of leading bytes read to identify a file by its contents.
pub const SNIFF_LEN: usize = 4096;
/// Whether hidden files are shown by default.
pub const SHOW_HIDDEN_DEFAULT: bool = false;
/// File name of the favorites store inside the application's config directory.
pub const FAVORITES_FILE_NAME: &str = "favorites.json";
/// Version number written into the favorites store.
pub const FAVORITES_FORMAT_VERSION: u32 = 1;

/// Extensions (lowercase, no dot) treated as images when content is unrecognised.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "webp", "gif", "bmp", "tif", "tiff", "avif",
];
/// Extensions (lowercase, no dot) treated as videos when content is unrecognised.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "mpg", "mpeg", "mpe", "wmv", "flv", "3gp", "ogv",
];

// ---------------------------------------------------------------------------
// User interface and video playback
// ---------------------------------------------------------------------------

/// Initial window size (width, height) in points.
pub const WINDOW_DEFAULT_SIZE: [f32; 2] = [1280.0, 800.0];
/// Minimum window size (width, height) in points.
pub const WINDOW_MIN_SIZE: [f32; 2] = [760.0, 480.0];
/// Width of the file-browser side panel in points.
pub const BROWSER_PANEL_WIDTH: f32 = 320.0;
/// Gray level (0-255) of the viewer background.
pub const VIEWER_BACKGROUND_GRAY: u8 = 24;
/// Seconds a transient status message stays visible.
pub const STATUS_MESSAGE_SECS: f64 = 8.0;
/// Seconds skipped by the video seek-step shortcuts.
pub const VIDEO_SEEK_STEP_SECS: f64 = 5.0;
/// Default video volume (0.0 to 1.0).
pub const VIDEO_DEFAULT_VOLUME: f64 = 1.0;
/// Volume change per keyboard step.
pub const VIDEO_VOLUME_STEP: f64 = 0.05;
/// Block size pushed into the video pipeline's application source.
pub const VIDEO_APPSRC_BLOCK_LEN: usize = 256 * 1024;
/// Maximum number of bytes queued inside the video application source.
pub const VIDEO_APPSRC_MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum number of decoded frames buffered by the video sink.
pub const VIDEO_SINK_MAX_BUFFERS: u32 = 2;
/// Interval (ms) at which the UI repaints while a video is playing.
pub const VIDEO_REPAINT_MS: u64 = 16;
/// Seconds to wait for the video pipeline to change state.
pub const VIDEO_STATE_TIMEOUT_SECS: u64 = 10;

#[cfg(test)]
mod tests;