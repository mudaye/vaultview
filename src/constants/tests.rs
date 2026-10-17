#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assertions_on_constants,
    clippy::const_is_empty
)]

use super::*;

#[test]
fn deep_delete_uses_three_passes() {
    assert_eq!(DEEP_DELETE_PASSES, 3);
}

#[test]
fn encrypted_extension_is_lowercase_and_dotless() {
    assert_eq!(ENCRYPTED_EXTENSION, "vvenc");
    assert!(!ENCRYPTED_EXTENSION.starts_with('.'));
    assert_eq!(
        ENCRYPTED_EXTENSION,
        ENCRYPTED_EXTENSION.to_ascii_lowercase()
    );
}

#[test]
fn temp_suffix_is_distinct_from_encrypted_extension() {
    assert!(!TEMP_SUFFIX.is_empty());
    assert!(!TEMP_SUFFIX.starts_with('.'));
    assert_ne!(TEMP_SUFFIX, ENCRYPTED_EXTENSION);
    assert!(!DECRYPTED_FALLBACK_EXTENSION.is_empty());
}

#[test]
fn magic_and_version_are_stable() {
    assert_eq!(MAGIC, *b"VVLT\x89\r\n\x1a");
    assert_eq!(FORMAT_VERSION, 1);
    assert_eq!(KDF_ID_ARGON2ID, 1);
    assert_eq!(AEAD_ID_XCHACHA20_POLY1305, 1);
    assert_eq!(COMPRESSION_NONE, 0);
    assert_eq!(COMPRESSION_DEFLATE, 1);
}

#[test]
fn chunk_size_matches_its_log2_and_bounds() {
    assert_eq!(CHUNK_SIZE, 1usize << CHUNK_SIZE_LOG2);
    assert!(CHUNK_SIZE_LOG2_MIN <= CHUNK_SIZE_LOG2);
    assert!(CHUNK_SIZE_LOG2 <= CHUNK_SIZE_LOG2_MAX);
}

#[test]
fn format_lengths_are_consistent() {
    assert_eq!(KEY_LEN, 32);
    assert_eq!(SALT_LEN, 16);
    assert_eq!(NONCE_LEN, 24);
    assert_eq!(TAG_LEN, 16);
    assert_eq!(WRAPPED_KEY_LEN, KEY_LEN + TAG_LEN);
    assert_eq!(NONCE_PREFIX_LEN + 8 + 1, NONCE_LEN);
    assert_eq!(
        HEADER_LEN,
        MAGIC.len() + 2 + 4 + 12 + SALT_LEN + NONCE_LEN + WRAPPED_KEY_LEN + NONCE_PREFIX_LEN
    );
    assert_eq!(HEADER_LEN, 129);
    assert_eq!(KEY_WRAP_AAD_LEN, MAGIC.len() + 2 + 4 + 12 + SALT_LEN);
    assert_eq!(KEY_WRAP_AAD_LEN, 42);
}

#[test]
fn default_kdf_parameters_are_within_bounds() {
    assert!((KDF_MEMORY_KIB_MIN..=KDF_MEMORY_KIB_MAX).contains(&KDF_MEMORY_KIB_DEFAULT));
    assert!((KDF_ITERATIONS_MIN..=KDF_ITERATIONS_MAX).contains(&KDF_ITERATIONS_DEFAULT));
    assert!((KDF_PARALLELISM_MIN..=KDF_PARALLELISM_MAX).contains(&KDF_PARALLELISM_DEFAULT));
    assert!(KDF_MEMORY_KIB_MIN >= 8);
    assert!(
        KDF_MEMORY_KIB_DEFAULT >= 19_456,
        "default memory cost must stay at a sane minimum"
    );
}

#[test]
fn operational_limits_are_sane() {
    assert!(SHRED_BUFFER_LEN > 0);
    assert!(SNIFF_LEN >= 64);
    assert!(MAX_SESSION_PASSWORDS >= 1);
    assert!(DECRYPTED_CACHE_MAX_BYTES > 0);
    assert!(MAX_DECRYPTED_IMAGE_BYTES > 0);
    assert!(MAX_IMAGE_PIXELS > 0);
    assert!(MAX_TEXTURE_SIDE >= 2048);
    assert!(MIN_PASSWORD_CHARS >= 8);
    assert!(COMPRESSION_SAMPLE_LEN > 0);
    assert!((1..100).contains(&COMPRESSION_MIN_SAVING_PERCENT));
    assert!(FRAME_DELAY_MIN_MS < FRAME_DELAY_DEFAULT_MS);
}

#[test]
fn zoom_limits_are_ordered() {
    assert!(ZOOM_MIN > 0.0 && ZOOM_MIN < 1.0);
    assert!(ZOOM_MAX > 1.0);
    assert!(ZOOM_STEP_FACTOR > 1.0);
}

#[test]
fn favorites_constants_are_defined() {
    assert!(FAVORITES_FILE_NAME.ends_with(".json"));
    assert_eq!(FAVORITES_FORMAT_VERSION, 1);
    assert!(!APP_NAME.is_empty());
}

#[test]
fn extension_lists_are_lowercase_dotless_and_disjoint() {
    for ext in IMAGE_EXTENSIONS.iter().chain(VIDEO_EXTENSIONS.iter()) {
        assert!(!ext.is_empty());
        assert!(!ext.starts_with('.'));
        assert_eq!(*ext, ext.to_ascii_lowercase());
    }
    for ext in IMAGE_EXTENSIONS {
        assert!(
            !VIDEO_EXTENSIONS.contains(ext),
            "{ext} listed as both image and video"
        );
    }
    for required in [
        "jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "avif",
    ] {
        assert!(
            IMAGE_EXTENSIONS.contains(&required),
            "missing image extension {required}"
        );
    }
    for required in ["mp4", "mov", "mkv", "webm", "avi", "mpg", "mpeg"] {
        assert!(
            VIDEO_EXTENSIONS.contains(&required),
            "missing video extension {required}"
        );
    }
    assert!(!IMAGE_EXTENSIONS.contains(&ENCRYPTED_EXTENSION));
    assert!(!VIDEO_EXTENSIONS.contains(&ENCRYPTED_EXTENSION));
}

#[test]
fn audio_and_pdf_extensions_are_well_formed_and_do_not_overlap_other_kinds() {
    for ext in AUDIO_EXTENSIONS {
        assert!(!ext.is_empty());
        assert!(!ext.starts_with('.'));
        assert_eq!(*ext, ext.to_ascii_lowercase());
        assert!(
            !IMAGE_EXTENSIONS.contains(ext),
            "{ext} listed as image and audio"
        );
        assert!(
            !VIDEO_EXTENSIONS.contains(ext),
            "{ext} listed as video and audio"
        );
        assert_ne!(*ext, PDF_EXTENSION);
        assert_ne!(*ext, ENCRYPTED_EXTENSION);
    }
    for required in ["mp3", "m4a", "aac", "flac", "ogg", "opus", "wav"] {
        assert!(
            AUDIO_EXTENSIONS.contains(&required),
            "missing audio extension {required}"
        );
    }
    assert_eq!(PDF_EXTENSION, "pdf");
    assert!(!IMAGE_EXTENSIONS.contains(&PDF_EXTENSION));
    assert!(!VIDEO_EXTENSIONS.contains(&PDF_EXTENSION));
}

#[test]
fn pdf_limits_are_sane() {
    assert!(MAX_PDF_BYTES > 0);
    assert!(PDF_MAX_RENDER_PIXELS >= u64::from(MAX_TEXTURE_SIDE));
    assert!(PDF_RENDER_STEPS_PER_OCTAVE >= 1.0);
    assert!(PDF_MIN_RENDER_SCALE > 0.0 && PDF_MIN_RENDER_SCALE < 1.0);
    assert!(PDF_DEFAULT_PAGE_SIZE.0 > 0.0 && PDF_DEFAULT_PAGE_SIZE.1 > 0.0);
    assert!(PDF_CONTROLS_HEIGHT > 0.0);
    assert!(PDF_POLL_MS > 0);
}

#[test]
fn audio_limits_are_sane() {
    assert!(AUDIO_TAG_MAX_CHARS >= 64);
    assert!(AUDIO_COVER_MAX_BYTES > 0);
    assert!(AUDIO_COVER_MAX_SIDE >= 64 && AUDIO_COVER_MAX_SIDE <= MAX_TEXTURE_SIDE);
    assert!(AUDIO_VIEW_MARGIN >= 0.0);
}

#[test]
fn theater_and_boss_key_constants_are_sane() {
    assert!(THEATER_HUD_TIMEOUT_SECS > 0.0);
    assert!(THEATER_HUD_MARGIN >= 0.0);
    assert!(
        BOSS_SETTLE_FRAMES >= 1,
        "at least one frame must pass between locking and minimizing"
    );
    assert!(BOSS_LEAVE_THEATER_TIMEOUT_FRAMES > 0);
    assert!(THEATER_SWITCH_TIMEOUT_FRAMES <= 600);
    assert!(THEATER_UNMAXIMIZE_TIMEOUT_FRAMES <= 600);
    assert!(!THEATER_STRATEGY_ENV.is_empty());
    assert_eq!(
        THEATER_STRATEGY_ENV,
        THEATER_STRATEGY_ENV.to_ascii_uppercase()
    );
    assert!(!THEATER_STRATEGY_ENV.contains(' '));
}
