//! Error type shared by every module.

use std::io;
use std::path::PathBuf;

use thiserror::Error as ThisError;

/// Convenient result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Every failure the application can report.
#[derive(Debug, ThisError)]
pub enum Error {
    /// An operating-system I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// The data does not start with the encrypted-file magic value.
    #[error("not an encrypted file (magic value missing)")]
    NotEncrypted,
    /// The file uses a format version this build cannot read.
    #[error("unsupported encrypted-file format version {0}")]
    UnsupportedVersion(u16),
    /// The header is truncated or contains an invalid field.
    #[error("invalid encrypted-file header: {0}")]
    InvalidHeader(&'static str),
    /// Key-derivation parameters are outside the permitted range.
    #[error("key-derivation parameters are outside the permitted range")]
    InvalidKdfParams,
    /// The password is wrong or the authenticated key header was modified.
    #[error("wrong password, or the file's key header was modified")]
    WrongPassword,
    /// No usable password is available and one must be entered.
    #[error("a password is required")]
    PasswordRequired,
    /// An empty password was supplied.
    #[error("the password must not be empty")]
    EmptyPassword,
    /// A new password is shorter than the policy minimum.
    #[error("the password must contain at least {min} characters")]
    PasswordTooShort {
        /// Minimum number of characters required.
        min: usize,
    },
    /// Authentication failed, or the file is truncated, extended or damaged.
    #[error("the encrypted file is corrupted, truncated or was tampered with")]
    Corrupted,
    /// The file is already in the encrypted format.
    #[error("the file is already encrypted")]
    AlreadyEncrypted,
    /// A destination path is already in use.
    #[error("destination already exists: {}", .0.display())]
    DestinationExists(PathBuf),
    /// The path is not a regular file.
    #[error("not a regular file: {}", .0.display())]
    NotAFile(PathBuf),
    /// The path is not a directory.
    #[error("not a directory: {}", .0.display())]
    NotADirectory(PathBuf),
    /// The requested feature or format is not supported.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Media could not be decoded.
    #[error("could not decode: {0}")]
    Decode(String),
    /// Content exceeds a configured size limit.
    #[error("too large (limit {limit} bytes)")]
    TooLarge {
        /// The limit that was exceeded, in bytes (or pixels for images).
        limit: u64,
    },
    /// A caller supplied an invalid argument.
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    /// Stored application data could not be read or written.
    #[error("could not read or write stored data: {0}")]
    Serialization(String),
    /// Key derivation failed.
    #[error("key derivation failed: {0}")]
    Kdf(String),
    /// A cryptographic primitive failed unexpectedly.
    #[error("cryptographic operation failed: {0}")]
    Crypto(&'static str),
}

impl From<Error> for io::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Io(inner) => inner,
            other => io::Error::new(io::ErrorKind::InvalidData, other),
        }
    }
}
