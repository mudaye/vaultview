//! Persistent list of favorite folders.
//!
//! The store contains only a version number and folder paths; it never holds
//! decrypted content or passwords. Paths that are not valid UTF-8 cannot be
//! stored (serialisation reports an error).

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::constants::{APP_NAME, FAVORITES_FILE_NAME, FAVORITES_FORMAT_VERSION};
use crate::error::{Error, Result};

/// Availability of a favorite folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FavoriteStatus {
    /// The folder exists and can be listed.
    Available,
    /// The folder no longer exists (moved, deleted, unmounted).
    Missing,
    /// The path exists but is not a directory.
    NotADirectory,
    /// The folder exists but cannot be accessed.
    Inaccessible,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    folders: Vec<PathBuf>,
}

/// An ordered list of favorite folders.
#[derive(Debug, Clone, Default)]
pub struct Favorites {
    folders: Vec<PathBuf>,
}

impl Favorites {
    /// Creates an empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an existing folder. Returns `false` if it was already a favorite.
    pub fn add(&mut self, folder: &Path) -> Result<bool> {
        let meta = fs::metadata(folder)?;
        if !meta.is_dir() {
            return Err(Error::NotADirectory(folder.to_path_buf()));
        }
        if self.contains(folder) {
            return Ok(false);
        }
        self.folders.push(folder.to_path_buf());
        Ok(true)
    }

    /// Removes a favorite (the folder itself is untouched).
    pub fn remove(&mut self, folder: &Path) -> bool {
        let before = self.folders.len();
        self.folders.retain(|f| f != folder);
        self.folders.len() != before
    }

    /// Whether `folder` is a favorite.
    pub fn contains(&self, folder: &Path) -> bool {
        self.folders.iter().any(|f| f == folder)
    }

    /// Favorite folders in insertion order.
    pub fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    /// Serialises the list to JSON.
    pub fn to_json(&self) -> Result<String> {
        let stored = Stored {
            version: FAVORITES_FORMAT_VERSION,
            folders: self.folders.clone(),
        };
        serde_json::to_string_pretty(&stored).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Parses a list from JSON, rejecting unknown versions.
    pub fn from_json(json: &str) -> Result<Self> {
        let stored: Stored =
            serde_json::from_str(json).map_err(|e| Error::Serialization(e.to_string()))?;
        if stored.version != FAVORITES_FORMAT_VERSION {
            return Err(Error::Serialization(format!(
                "unsupported favorites version {}",
                stored.version
            )));
        }
        Ok(Self {
            folders: stored.folders,
        })
    }

    /// Loads the list from `path`. A missing file yields an empty list; a
    /// corrupt file is an error and is left untouched.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(json) => Self::from_json(&json),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Saves the list atomically (temporary file, then rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = self.to_json()?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut temp_name = path.as_os_str().to_owned();
        temp_name.push(".tmp");
        let temp = PathBuf::from(temp_name);
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&temp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(Error::Io)
    }
}

/// Reports whether a favorite folder is currently usable.
pub fn status(folder: &Path) -> FavoriteStatus {
    match fs::metadata(folder) {
        Ok(meta) if meta.is_dir() => {
            if fs::read_dir(folder).is_ok() {
                FavoriteStatus::Available
            } else {
                FavoriteStatus::Inaccessible
            }
        }
        Ok(_) => FavoriteStatus::NotADirectory,
        Err(e) if e.kind() == ErrorKind::NotFound => FavoriteStatus::Missing,
        Err(_) => FavoriteStatus::Inaccessible,
    }
}

/// Default location of the favorites store in the user's configuration directory.
pub fn default_store_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join(APP_NAME).join(FAVORITES_FILE_NAME))
}

#[cfg(test)]
mod tests;
