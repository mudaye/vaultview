//! Encrypted-extension and filename handling.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::constants::{DECRYPTED_FALLBACK_EXTENSION, ENCRYPTED_EXTENSION, TEMP_SUFFIX};

/// Where a decrypted file should be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// The original name was recovered and is free to use.
    Restored(PathBuf),
    /// The user must confirm or choose a name; a safe suggestion is provided.
    NeedsChoice {
        /// A currently unused name next to the encrypted file.
        suggested: PathBuf,
    },
}

/// Returns `true` when something (file, directory or dangling link) occupies `path`.
pub fn is_taken(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Appends the encrypted extension, keeping the original name and extension.
pub fn encrypted_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".");
    name.push(ENCRYPTED_EXTENSION);
    PathBuf::from(name)
}

/// Returns `true` when the path ends in the encrypted extension (case-insensitive).
pub fn has_encrypted_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(ENCRYPTED_EXTENSION))
}

/// Removes the encrypted extension, returning `None` when it is absent.
pub fn original_path(path: &Path) -> Option<PathBuf> {
    has_encrypted_extension(path).then(|| path.with_extension(""))
}

/// Name of the hidden staging file used while encrypting `path` in place.
pub fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{TEMP_SUFFIX}"))
}

/// Returns `path` if unused, otherwise `stem (n).ext` for the first free `n`.
pub fn unique_path(path: &Path) -> PathBuf {
    if !is_taken(path) {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 1u32.. {
        let name = match &ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = path.with_file_name(name);
        if !is_taken(&candidate) {
            return candidate;
        }
    }
    path.to_path_buf()
}

/// Decides where decrypted content may be written without overwriting anything.
pub fn decryption_destination(encrypted: &Path) -> Destination {
    match original_path(encrypted) {
        Some(original) if !is_taken(&original) => Destination::Restored(original),
        Some(original) => Destination::NeedsChoice { suggested: unique_path(&original) },
        None => {
            let mut name: OsString = encrypted.as_os_str().to_owned();
            name.push(".");
            name.push(DECRYPTED_FALLBACK_EXTENSION);
            Destination::NeedsChoice { suggested: unique_path(&PathBuf::from(name)) }
        }
    }
}

#[cfg(test)]
mod tests;