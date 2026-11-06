//! Directory listing, content-based classification, navigation and selection.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::constants::{
    AUDIO_EXTENSIONS, IMAGE_EXTENSIONS, PDF_EXTENSION, SNIFF_LEN, VIDEO_EXTENSIONS,
};
use crate::error::Result;
use crate::format::is_encrypted_prefix;

/// What a directory entry is, determined from its contents where possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A directory.
    Directory,
    /// A recognised image.
    Image,
    /// A recognised video.
    Video,
    /// A recognised audio file.
    Audio,
    /// A PDF document.
    Pdf,
    /// A file in the application's encrypted format.
    Encrypted,
    /// Anything else.
    Other,
}

/// One row of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Full path.
    pub path: PathBuf,
    /// File name for display.
    pub name: String,
    /// Classification.
    pub kind: EntryKind,
    /// Size in bytes (0 for directories).
    pub size: u64,
}

impl Entry {
    /// Whether the entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}

/// Classifies a file from its leading bytes; the extension is only a fallback
/// for content that is not recognised at all.
pub fn classify_bytes(prefix: &[u8], extension: Option<&str>) -> EntryKind {
    if is_encrypted_prefix(prefix) {
        return EntryKind::Encrypted;
    }
    if let Some(detected) = infer::get(prefix) {
        return match detected.matcher_type() {
            infer::MatcherType::Image => EntryKind::Image,
            infer::MatcherType::Video => EntryKind::Video,
            infer::MatcherType::Audio => EntryKind::Audio,
            _ if detected.mime_type() == "application/pdf" => EntryKind::Pdf,
            _ => EntryKind::Other,
        };
    }
    match extension.map(str::to_ascii_lowercase) {
        Some(ext) if IMAGE_EXTENSIONS.contains(&ext.as_str()) => EntryKind::Image,
        Some(ext) if VIDEO_EXTENSIONS.contains(&ext.as_str()) => EntryKind::Video,
        Some(ext) if AUDIO_EXTENSIONS.contains(&ext.as_str()) => EntryKind::Audio,
        Some(ext) if ext == PDF_EXTENSION => EntryKind::Pdf,
        _ => EntryKind::Other,
    }
}

/// Reads the start of a file and classifies it.
pub fn classify_file(path: &Path) -> Result<EntryKind> {
    let file = File::open(path)?;
    let mut prefix = Vec::with_capacity(SNIFF_LEN);
    file.take(SNIFF_LEN as u64).read_to_end(&mut prefix)?;
    let extension = path.extension().and_then(|e| e.to_str());
    Ok(classify_bytes(&prefix, extension))
}

/// Case-insensitive "natural" ordering: digit runs compare by numeric value.
/// Ties are broken by plain string comparison so the order is total.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let ca: Vec<char> = a.to_lowercase().chars().collect();
    let cb: Vec<char> = b.to_lowercase().chars().collect();
    let (mut i, mut j) = (0, 0);
    loop {
        match (ca.get(i), cb.get(j)) {
            (None, None) => break,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&c), Some(&d)) => {
                if c.is_ascii_digit() && d.is_ascii_digit() {
                    let end_i = i + ca[i..].iter().take_while(|ch| ch.is_ascii_digit()).count();
                    let end_j = j + cb[j..].iter().take_while(|ch| ch.is_ascii_digit()).count();
                    let run_a: Vec<char> = ca[i..end_i]
                        .iter()
                        .copied()
                        .skip_while(|ch| *ch == '0')
                        .collect();
                    let run_b: Vec<char> = cb[j..end_j]
                        .iter()
                        .copied()
                        .skip_while(|ch| *ch == '0')
                        .collect();
                    let order = run_a
                        .len()
                        .cmp(&run_b.len())
                        .then_with(|| run_a.cmp(&run_b));
                    if order != Ordering::Equal {
                        return order;
                    }
                    i = end_i;
                    j = end_j;
                } else {
                    match c.cmp(&d) {
                        Ordering::Equal => {
                            i += 1;
                            j += 1;
                        }
                        other => return other,
                    }
                }
            }
        }
    }
    a.cmp(b)
}

#[cfg(windows)]
fn has_hidden_attribute(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(windows))]
fn has_hidden_attribute(_meta: &fs::Metadata) -> bool {
    false
}

/// Lists a directory: directories first, then files in natural name order.
///
/// Entries that cannot be inspected are listed as [`EntryKind::Other`] rather
/// than aborting the listing.
pub fn list_directory(dir: &Path, show_hidden: bool) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        let meta = fs::metadata(&path).ok();
        if !show_hidden
            && (name.starts_with('.') || meta.as_ref().is_some_and(has_hidden_attribute))
        {
            continue;
        }
        let (kind, size) = match &meta {
            Some(m) if m.is_dir() => (EntryKind::Directory, 0),
            Some(m) => (classify_file(&path).unwrap_or(EntryKind::Other), m.len()),
            None => (EntryKind::Other, 0),
        };
        entries.push(Entry {
            path,
            name,
            kind,
            size,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir()
            .cmp(&a.is_dir())
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
    Ok(entries)
}

/// Previous/next navigation over the viewable files of one folder listing.
///
/// Peeking at the neighbours never moves the cursor, so a failed open (for
/// example a wrong password) cannot silently advance to another file.
#[derive(Debug, Clone, Default)]
pub struct NavContext {
    items: Vec<PathBuf>,
    current: Option<usize>,
}

impl NavContext {
    /// Builds a context from a listing: images, videos, audio files, PDFs and
    /// encrypted files participate.
    pub fn from_entries(entries: &[Entry]) -> Self {
        let items = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EntryKind::Image
                        | EntryKind::Video
                        | EntryKind::Audio
                        | EntryKind::Pdf
                        | EntryKind::Encrypted
                )
            })
            .map(|e| e.path.clone())
            .collect();
        Self {
            items,
            current: None,
        }
    }

    /// Makes `path` the current file. Returns `false` (changing nothing) when
    /// the path does not participate in navigation.
    pub fn set_current(&mut self, path: &Path) -> bool {
        match self.items.iter().position(|p| p == path) {
            Some(index) => {
                self.current = Some(index);
                true
            }
            None => false,
        }
    }

    /// The current file, if any.
    pub fn current(&self) -> Option<&Path> {
        self.current
            .and_then(|i| self.items.get(i))
            .map(PathBuf::as_path)
    }

    /// The file after the current one, without moving.
    pub fn next_path(&self) -> Option<&Path> {
        let index = self.current? + 1;
        self.items.get(index).map(PathBuf::as_path)
    }

    /// The file before the current one, without moving.
    pub fn previous_path(&self) -> Option<&Path> {
        let index = self.current?.checked_sub(1)?;
        self.items.get(index).map(PathBuf::as_path)
    }

    /// One-based position of the current file and the number of files.
    pub fn position(&self) -> Option<(usize, usize)> {
        self.current.map(|i| (i + 1, self.items.len()))
    }

    /// Number of navigable files.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no navigable files.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// A set of selected files (directories can never be selected).
#[derive(Debug, Clone, Default)]
pub struct Selection {
    paths: BTreeSet<PathBuf>,
}

impl Selection {
    /// Toggles a file in or out of the selection. Returns `false` for
    /// directories, which are never selectable.
    pub fn toggle(&mut self, entry: &Entry) -> bool {
        if entry.is_dir() {
            return false;
        }
        if !self.paths.remove(&entry.path) {
            self.paths.insert(entry.path.clone());
        }
        true
    }

    /// Selects the files between two listing indices (inclusive, any order).
    pub fn select_range(&mut self, entries: &[Entry], from: usize, to: usize) {
        if entries.is_empty() {
            return;
        }
        let last = entries.len() - 1;
        let (lo, hi) = (from.min(to).min(last), from.max(to).min(last));
        for entry in &entries[lo..=hi] {
            if !entry.is_dir() {
                self.paths.insert(entry.path.clone());
            }
        }
    }

    /// Selects every file in the listing.
    pub fn select_all_files(&mut self, entries: &[Entry]) {
        for entry in entries.iter().filter(|e| !e.is_dir()) {
            self.paths.insert(entry.path.clone());
        }
    }

    /// Clears the selection.
    pub fn clear(&mut self) {
        self.paths.clear();
    }

    /// Whether `path` is selected.
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    /// Selected paths in sorted order.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.paths.iter().cloned().collect()
    }

    /// Number of selected files.
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// The selected files that may be encrypted (not directories, not already
/// encrypted), in listing order.
pub fn encryptable_selection(selection: &Selection, entries: &[Entry]) -> Vec<PathBuf> {
    entries
        .iter()
        .filter(|e| !e.is_dir() && e.kind != EntryKind::Encrypted && selection.contains(&e.path))
        .map(|e| e.path.clone())
        .collect()
}

#[cfg(test)]
mod tests;
