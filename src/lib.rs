//! VaultView: a cross-platform media viewer with integrated password-based
//! file encryption, favorites and privacy-conscious file management.
//!
//! The library holds all logic (and all unit tests); `main.rs` only starts
//! the GUI. The encrypted-file format is documented in `docs/FORMAT.md`.

pub mod app;
pub mod browser;
pub mod constants;
pub mod crypto;
pub mod error;
pub mod favorites;
pub mod format;
pub mod media;
pub mod naming;
pub mod session;
pub mod shred;
#[cfg(feature = "video")]
pub mod video;
pub mod viewer;
pub mod workflow;

pub use error::{Error, Result};