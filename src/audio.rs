//! Audio metadata collected from stream tags (pure data; no media library).

use crate::constants::{AUDIO_COVER_MAX_BYTES, AUDIO_TAG_MAX_CHARS};

/// Descriptive information about an audio file, gathered while it plays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioInfo {
    /// Track title.
    pub title: Option<String>,
    /// Performing artist.
    pub artist: Option<String>,
    /// Album name.
    pub album: Option<String>,
    /// Genre.
    pub genre: Option<String>,
    /// Audio codec description.
    pub codec: Option<String>,
    /// Bit rate in bits per second.
    pub bitrate_bps: Option<u32>,
    /// Embedded cover art (encoded image bytes), until taken for display.
    pub cover_art: Option<Vec<u8>>,
}

/// Trims a tag value, drops control characters and limits its length.
/// Returns `None` when nothing meaningful remains.
pub fn clean_tag(value: &str) -> Option<String> {
    let cleaned: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(AUDIO_TAG_MAX_CHARS)
        .collect();
    let trimmed = cleaned.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn fill(slot: &mut Option<String>, value: &str) {
    if slot.is_none() {
        *slot = clean_tag(value);
    }
}

/// Formats a bit rate such as `320 kbps`.
pub fn format_bitrate(bits_per_second: u32) -> String {
    format!("{} kbps", (bits_per_second + 500) / 1000)
}

impl AudioInfo {
    /// Sets the title unless one is already known.
    pub fn set_title(&mut self, value: &str) {
        fill(&mut self.title, value);
    }

    /// Sets the artist unless one is already known.
    pub fn set_artist(&mut self, value: &str) {
        fill(&mut self.artist, value);
    }

    /// Sets the album unless one is already known.
    pub fn set_album(&mut self, value: &str) {
        fill(&mut self.album, value);
    }

    /// Sets the genre unless one is already known.
    pub fn set_genre(&mut self, value: &str) {
        fill(&mut self.genre, value);
    }

    /// Sets the codec unless one is already known.
    pub fn set_codec(&mut self, value: &str) {
        fill(&mut self.codec, value);
    }

    /// Records a bit rate; zero is ignored and later values replace earlier ones.
    pub fn set_bitrate(&mut self, bits_per_second: u32) {
        if bits_per_second > 0 {
            self.bitrate_bps = Some(bits_per_second);
        }
    }

    /// Keeps the first non-empty cover image that is not too large.
    pub fn set_cover_art(&mut self, bytes: &[u8]) {
        if self.cover_art.is_none() && !bytes.is_empty() && bytes.len() <= AUDIO_COVER_MAX_BYTES {
            self.cover_art = Some(bytes.to_vec());
        }
    }

    /// Hands out the cover image bytes once.
    pub fn take_cover_art(&mut self) -> Option<Vec<u8>> {
        self.cover_art.take()
    }

    /// The title, or `fallback` (typically the file name) when unknown.
    pub fn title_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.title.as_deref().unwrap_or(fallback)
    }

    /// A one-line technical summary such as `FLAC · 320 kbps`.
    pub fn details_line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(codec) = &self.codec {
            parts.push(codec.clone());
        }
        if let Some(bitrate) = self.bitrate_bps {
            parts.push(format_bitrate(bitrate));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    /// Whether any descriptive text is known.
    pub fn has_text(&self) -> bool {
        self.title.is_some()
            || self.artist.is_some()
            || self.album.is_some()
            || self.genre.is_some()
            || self.details_line().is_some()
    }
}

#[cfg(test)]
mod tests;
