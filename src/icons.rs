//! Platform-neutral icon definitions using bundled Lucide font glyphs.

/// Folder icon for directories.
pub const FOLDER: &str = "\u{E0D7}";
/// Image icon for pictures and graphic files.
pub const IMAGE: &str = "\u{E0F6}";
/// Video icon for film and animation files.
pub const VIDEO: &str = "\u{E1A5}";
/// Audio icon for music and sound files.
pub const AUDIO: &str = "\u{E122}";
/// PDF icon for documents.
pub const PDF: &str = "\u{E0CC}";
/// Lock icon for encrypted files and security states.
pub const LOCK: &str = "\u{E10B}";
/// Unlock icon for decryption actions.
pub const UNLOCK: &str = "\u{E10C}";
/// Key icon for password management.
pub const KEY: &str = "\u{E0FD}";
/// Generic file icon for unrecognized content.
pub const FILE: &str = "\u{E0C0}";
/// Star icon for favorite items.
pub const STAR: &str = "\u{E176}";
/// Minus icon for zoom out.
pub const ZOOM_OUT: &str = "\u{E11C}";
/// Plus icon for zoom in.
pub const ZOOM_IN: &str = "\u{E13D}";
/// Chevron left icon for previous file or page.
pub const CHEVRON_LEFT: &str = "\u{E06E}";
/// Chevron right icon for next file or page.
pub const CHEVRON_RIGHT: &str = "\u{E06F}";
/// Skip back icon for first page.
pub const SKIP_BACK: &str = "\u{E15F}";
/// Skip forward icon for last page.
pub const SKIP_FORWARD: &str = "\u{E160}";
/// Arrow up icon for directory navigation.
pub const ARROW_UP: &str = "\u{E04A}";
/// Trash icon for standard deletion.
pub const TRASH: &str = "\u{E18D}";
/// Trash-2 icon for deep deletion.
pub const TRASH_2: &str = "\u{E18E}";
/// Close / dismiss icon.
pub const X: &str = "\u{E1B2}";
/// Database icon for decrypted cache.
pub const DATABASE: &str = "\u{E0AD}";
/// Shield icon for privacy and security lockdown.
pub const SHIELD: &str = "\u{E158}";
/// Maximize icon for theater mode.
pub const MAXIMIZE: &str = "\u{E112}";
/// Help circle icon.
pub const HELP_CIRCLE: &str = "\u{E082}";
/// Check icon for selection.
pub const CHECK: &str = "\u{E06C}";
/// Empty square icon for clearing selection.
pub const SQUARE: &str = "\u{E167}";
/// Refresh icon.
pub const REFRESH_CW: &str = "\u{E145}";
/// Home directory icon.
pub const HOME: &str = "\u{E0F5}";

/// Embedded Lucide icon font bytes.
pub const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");
