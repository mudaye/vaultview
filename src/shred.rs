//! Normal deletion and deep deletion (multi-pass overwrite, then delete).
//!
//! Overwriting logical file contents does **not** guarantee physical erasure
//! on SSDs, wear-levelled media, journaling or copy-on-write filesystems,
//! snapshots or backups. See the user guide.

use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use rand::rngs::OsRng;
use rand::RngCore;

use crate::constants::{DEEP_DELETE_PASSES, SHRED_BUFFER_LEN};
use crate::error::{Error, Result};

/// What a completed deep deletion did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShredReport {
    /// Number of overwrite passes that completed.
    pub passes: u32,
    /// Number of bytes overwritten in each pass.
    pub bytes_per_pass: u64,
}

/// Deletes a file using the operating system's standard behaviour.
pub fn normal_delete(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        return Err(Error::NotAFile(path.to_path_buf()));
    }
    fs::remove_file(path)?;
    Ok(())
}

/// Overwrites a file [`DEEP_DELETE_PASSES`] times with random data, then deletes it.
pub fn deep_delete(path: &Path) -> Result<ShredReport> {
    deep_delete_observed(path, DEEP_DELETE_PASSES, &mut |_, _| {})
}

/// Like [`deep_delete`] with an explicit pass count and an observer that is
/// called after each pass has been written and synced.
///
/// Success is only reported when every pass and the final deletion succeeded.
/// Symbolic links are refused so that a link can never cause its target to be
/// shredded.
pub fn deep_delete_observed(
    path: &Path,
    passes: u32,
    observer: &mut dyn FnMut(u32, &Path),
) -> Result<ShredReport> {
    if passes == 0 {
        return Err(Error::InvalidArgument("at least one overwrite pass is required"));
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() {
        return Err(Error::NotAFile(path.to_path_buf()));
    }
    let mut file = OpenOptions::new().write(true).open(path)?;
    let len = file.metadata()?.len();
    let buffer_len = usize::try_from(len.min(SHRED_BUFFER_LEN as u64)).unwrap_or(SHRED_BUFFER_LEN);
    let mut buffer = vec![0u8; buffer_len];

    for pass in 1..=passes {
        file.seek(SeekFrom::Start(0))?;
        let mut remaining = len;
        while remaining > 0 {
            let n = usize::try_from(remaining.min(buffer_len as u64)).unwrap_or(buffer_len);
            OsRng
                .try_fill_bytes(&mut buffer[..n])
                .map_err(|_| Error::Crypto("system random number generator unavailable"))?;
            file.write_all(&buffer[..n])?;
            remaining -= n as u64;
        }
        file.flush()?;
        file.sync_all()?;
        observer(pass, path);
    }

    drop(file);
    fs::remove_file(path)?;
    Ok(ShredReport { passes, bytes_per_pass: len })
}

#[cfg(test)]
mod tests;