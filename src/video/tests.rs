#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

fn tag_list(fill: impl FnOnce(&mut gst::TagListRef)) -> gst::TagList {
    gst::init().unwrap();
    let mut tags = gst::TagList::new();
    fill(tags.get_mut().unwrap());
    tags
}

fn image_sample(bytes: &[u8]) -> gst::Sample {
    gst::Sample::builder()
        .buffer(&gst::Buffer::from_slice(bytes.to_vec()))
        .build()
}

#[test]
fn descriptive_tags_are_copied_into_the_audio_info() {
    let tags = tag_list(|t| {
        t.add::<gst::tags::Title>(&"Song", gst::TagMergeMode::Append);
        t.add::<gst::tags::Artist>(&"Artist", gst::TagMergeMode::Append);
        t.add::<gst::tags::Album>(&"Album", gst::TagMergeMode::Append);
        t.add::<gst::tags::Genre>(&"Jazz", gst::TagMergeMode::Append);
        t.add::<gst::tags::AudioCodec>(&"FLAC", gst::TagMergeMode::Append);
        t.add::<gst::tags::Bitrate>(&192_000u32, gst::TagMergeMode::Append);
    });
    let mut info = AudioInfo::default();
    merge_tags(&mut info, &tags);
    assert_eq!(info.title.as_deref(), Some("Song"));
    assert_eq!(info.artist.as_deref(), Some("Artist"));
    assert_eq!(info.album.as_deref(), Some("Album"));
    assert_eq!(info.genre.as_deref(), Some("Jazz"));
    assert_eq!(info.codec.as_deref(), Some("FLAC"));
    assert_eq!(info.bitrate_bps, Some(192_000));
    assert_eq!(info.cover_art, None);
}

#[test]
fn later_tag_messages_do_not_overwrite_known_text() {
    let first = tag_list(|t| {
        t.add::<gst::tags::Title>(&"First", gst::TagMergeMode::Append);
    });
    let second = tag_list(|t| {
        t.add::<gst::tags::Title>(&"Second", gst::TagMergeMode::Append);
        t.add::<gst::tags::Artist>(&"Late Artist", gst::TagMergeMode::Append);
    });
    let mut info = AudioInfo::default();
    merge_tags(&mut info, &first);
    merge_tags(&mut info, &second);
    assert_eq!(info.title.as_deref(), Some("First"));
    assert_eq!(info.artist.as_deref(), Some("Late Artist"));
}

#[test]
fn the_nominal_bitrate_is_used_when_there_is_no_bitrate() {
    let tags = tag_list(|t| {
        t.add::<gst::tags::NominalBitrate>(&128_000u32, gst::TagMergeMode::Append);
    });
    let mut info = AudioInfo::default();
    merge_tags(&mut info, &tags);
    assert_eq!(info.bitrate_bps, Some(128_000));
}

#[test]
fn cover_art_bytes_come_from_the_image_tag() {
    let tags = tag_list(|t| {
        t.add::<gst::tags::Image>(&image_sample(&[1, 2, 3, 4]), gst::TagMergeMode::Append);
    });
    let mut info = AudioInfo::default();
    merge_tags(&mut info, &tags);
    assert_eq!(info.cover_art.as_deref(), Some(&[1u8, 2, 3, 4][..]));
}

#[test]
fn the_preview_image_is_the_fallback_cover() {
    let tags = tag_list(|t| {
        t.add::<gst::tags::PreviewImage>(&image_sample(&[9, 8, 7]), gst::TagMergeMode::Append);
    });
    let mut info = AudioInfo::default();
    merge_tags(&mut info, &tags);
    assert_eq!(info.cover_art.as_deref(), Some(&[9u8, 8, 7][..]));
}

#[test]
fn an_empty_tag_list_changes_nothing() {
    let tags = tag_list(|_| {});
    let mut info = AudioInfo::default();
    merge_tags(&mut info, &tags);
    assert_eq!(info, AudioInfo::default());
}

#[test]
fn memory_sources_read_and_seek_like_files() {
    let mut source = MemorySource::new(Zeroizing::new((0u8..100).collect()));
    let mut buf = [0u8; 10];
    source.seek(SeekFrom::Start(20)).unwrap();
    source.read_exact(&mut buf).unwrap();
    assert_eq!(buf, [20, 21, 22, 23, 24, 25, 26, 27, 28, 29]);
    assert_eq!(source.seek(SeekFrom::End(-5)).unwrap(), 95);
    let mut tail = Vec::new();
    source.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, vec![95, 96, 97, 98, 99]);
    assert!(source.seek(SeekFrom::Current(-1000)).is_err());
}
