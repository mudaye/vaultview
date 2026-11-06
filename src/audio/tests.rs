#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::{AUDIO_COVER_MAX_BYTES, AUDIO_TAG_MAX_CHARS};

#[test]
fn tags_are_trimmed_cleaned_and_limited() {
    assert_eq!(clean_tag("  Hello  ").as_deref(), Some("Hello"));
    assert_eq!(clean_tag("a\u{0}b\nc\td").as_deref(), Some("abcd"));
    assert_eq!(clean_tag(""), None);
    assert_eq!(clean_tag("   \n\t "), None);
    let long = "x".repeat(AUDIO_TAG_MAX_CHARS + 100);
    assert_eq!(
        clean_tag(&long).unwrap().chars().count(),
        AUDIO_TAG_MAX_CHARS
    );
    assert_eq!(clean_tag("日本語の曲").as_deref(), Some("日本語の曲"));
}

#[test]
fn the_first_known_value_of_each_text_field_wins() {
    let mut info = AudioInfo::default();
    info.set_title("First");
    info.set_title("Second");
    info.set_artist("Artist");
    info.set_album("Album");
    info.set_genre("Jazz");
    info.set_codec("FLAC");
    assert_eq!(info.title.as_deref(), Some("First"));
    assert_eq!(info.artist.as_deref(), Some("Artist"));
    assert_eq!(info.album.as_deref(), Some("Album"));
    assert_eq!(info.genre.as_deref(), Some("Jazz"));
    assert_eq!(info.codec.as_deref(), Some("FLAC"));
}

#[test]
fn blank_values_do_not_block_later_real_ones() {
    let mut info = AudioInfo::default();
    info.set_title("   ");
    assert_eq!(info.title, None);
    info.set_title("Real");
    assert_eq!(info.title.as_deref(), Some("Real"));
}

#[test]
fn bitrate_ignores_zero_and_keeps_the_latest_value() {
    let mut info = AudioInfo::default();
    info.set_bitrate(0);
    assert_eq!(info.bitrate_bps, None);
    info.set_bitrate(128_000);
    info.set_bitrate(320_000);
    info.set_bitrate(0);
    assert_eq!(info.bitrate_bps, Some(320_000));
}

#[test]
fn bitrates_are_formatted_in_kilobits() {
    assert_eq!(format_bitrate(320_000), "320 kbps");
    assert_eq!(format_bitrate(128_499), "128 kbps");
    assert_eq!(format_bitrate(128_500), "129 kbps");
    assert_eq!(format_bitrate(0), "0 kbps");
}

#[test]
fn cover_art_is_kept_once_limited_in_size_and_taken_once() {
    let mut info = AudioInfo::default();
    info.set_cover_art(&[]);
    assert_eq!(info.cover_art, None);
    info.set_cover_art(&vec![0u8; AUDIO_COVER_MAX_BYTES + 1]);
    assert_eq!(info.cover_art, None);
    info.set_cover_art(&[1, 2, 3]);
    info.set_cover_art(&[9, 9]);
    assert_eq!(info.cover_art.as_deref(), Some(&[1u8, 2, 3][..]));
    assert_eq!(info.take_cover_art(), Some(vec![1, 2, 3]));
    assert_eq!(info.take_cover_art(), None);
}

#[test]
fn the_title_falls_back_to_the_given_name() {
    let mut info = AudioInfo::default();
    assert_eq!(info.title_or("song"), "song");
    info.set_title("Real Title");
    assert_eq!(info.title_or("song"), "Real Title");
}

#[test]
fn the_details_line_joins_what_is_known() {
    let mut info = AudioInfo::default();
    assert_eq!(info.details_line(), None);
    info.set_codec("MP3");
    assert_eq!(info.details_line().as_deref(), Some("MP3"));
    info.set_bitrate(192_000);
    assert_eq!(info.details_line().as_deref(), Some("MP3 · 192 kbps"));
    let mut only_rate = AudioInfo::default();
    only_rate.set_bitrate(64_000);
    assert_eq!(only_rate.details_line().as_deref(), Some("64 kbps"));
}

#[test]
fn has_text_reflects_any_descriptive_field() {
    let mut info = AudioInfo::default();
    assert!(!info.has_text());
    info.set_cover_art(&[1]);
    assert!(!info.has_text(), "cover art alone is not text");
    info.set_bitrate(128_000);
    assert!(info.has_text());
}
