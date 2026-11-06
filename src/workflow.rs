//! In-place batch encryption with explicit per-file outcomes.
//!
//! For each file the sequence is:
//!
//! 1. validate, then write a complete ciphertext to a hidden sibling staging file;
//! 2. fully re-verify the staging file (password, authentication, length);
//! 3. overwrite the original file's bytes with the ciphertext and sync;
//! 4. confirm the result is identical to the verified copy, rename it to
//!    `<name>.<ext>` and remove the staging file.
//!
//! A failure before step 3 leaves the original untouched. A failure during or
//! after step 3 is reported as [`Outcome::Indeterminate`] and the verified
//! staging copy is kept so no data is lost.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::constants::{FORMAT_VERSION_V1, IO_BLOCK_LEN, MIN_PASSWORD_CHARS};
use crate::crypto::{
    decrypt_stream, encrypt_stream, rewrap_header, Decryptor, EncryptOptions, EncryptStats,
};
use crate::error::{Error, Result};
use crate::format::{self, is_encrypted_file, KdfParams};
use crate::naming::{
    decrypted_temp_path_for, decryption_destination, encrypted_path, header_backup_path_for,
    is_taken, temp_path_for, Destination,
};
use crate::shred;
use std::io::Write;

/// Result of encrypting one file.
#[derive(Debug)]
pub enum Outcome {
    /// The file was decrypted and written to disk.
    Decrypted {
        /// Path of the decrypted plaintext file.
        output: PathBuf,
    },
    /// The file's password was changed.
    PasswordChanged {
        /// Path of the encrypted file whose password was updated.
        output: PathBuf,
    },
    /// The file was encrypted, verified and renamed.
    Encrypted {
        /// Path of the encrypted file.
        output: PathBuf,
    },
    /// Nothing was changed; the original file is intact.
    Failed {
        /// Why the operation failed.
        error: Error,
    },
    /// The original's contents may have been replaced but completion could not be confirmed.
    Indeterminate {
        /// What went wrong.
        error: Error,
        /// A complete verified encrypted copy, if one was kept.
        recovery_copy: Option<PathBuf>,
    },
}

/// Outcome for one file of a batch.
#[derive(Debug)]
pub struct FileReport {
    /// The file as it was selected.
    pub path: PathBuf,
    /// What happened to it.
    pub outcome: Outcome,
}

impl FileReport {
    /// A human-readable one-line description identifying the affected file.
    pub fn message(&self) -> String {
        match &self.outcome {
            Outcome::Decrypted { output } => {
                format!("Decrypted {} -> {}", self.path.display(), output.display())
            }
            Outcome::PasswordChanged { output } => {
                format!("Password changed for {}", output.display())
            }
            Outcome::Encrypted { output } => {
                format!("Encrypted {} -> {}", self.path.display(), output.display())
            }
            Outcome::Failed { error } => {
                format!("Failed: {}: {} (the file was not modified)", self.path.display(), error)
            }
            Outcome::Indeterminate { error, recovery_copy: Some(copy) } => format!(
                "UNCERTAIN: {}: {}. Its contents were replaced but completion could not be confirmed; \
                 a complete verified encrypted copy was kept at {}",
                self.path.display(),
                error,
                copy.display()
            ),
            Outcome::Indeterminate { error, recovery_copy: None } => format!(
                "UNCERTAIN: {}: {}. Its contents may have been replaced and no recovery copy exists",
                self.path.display(),
                error
            ),
        }
    }
}

/// Outcomes of every file in a batch, in selection order.
#[derive(Debug, Default)]
pub struct BatchReport {
    /// One report per selected file.
    pub files: Vec<FileReport>,
}

impl BatchReport {
    /// Number of files that were encrypted successfully.
    pub fn succeeded(&self) -> usize {
        self.files
            .iter()
            .filter(|f| {
                matches!(
                    f.outcome,
                    Outcome::Encrypted { .. }
                        | Outcome::Decrypted { .. }
                        | Outcome::PasswordChanged { .. }
                )
            })
            .count()
    }

    /// Number of files that failed without being modified.
    pub fn failed(&self) -> usize {
        self.files
            .iter()
            .filter(|f| matches!(f.outcome, Outcome::Failed { .. }))
            .count()
    }

    /// Number of files whose final state could not be confirmed.
    pub fn indeterminate(&self) -> usize {
        self.files
            .iter()
            .filter(|f| matches!(f.outcome, Outcome::Indeterminate { .. }))
            .count()
    }

    /// `true` when the batch is non-empty and every file succeeded.
    pub fn all_succeeded(&self) -> bool {
        !self.files.is_empty() && self.succeeded() == self.files.len()
    }
}

/// Checks the password policy for a new encryption batch.
pub fn validate_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        Err(Error::PasswordTooShort {
            min: MIN_PASSWORD_CHARS,
        })
    } else {
        Ok(())
    }
}

/// Test seams for simulating failures between the critical steps.
trait Hooks {
    fn after_verify(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn after_overwrite(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct NoHooks;

impl Hooks for NoHooks {}

enum Stop {
    Failed(Error),
    Indeterminate {
        error: Error,
        recovery: Option<PathBuf>,
    },
}

fn uncertain(error: Error, temp: &Path) -> Stop {
    Stop::Indeterminate {
        error,
        recovery: Some(temp.to_path_buf()),
    }
}

/// Encrypts one file in place. The password policy is enforced here too.
pub fn encrypt_one(path: &Path, password: &str, options: &EncryptOptions) -> Outcome {
    encrypt_one_with(path, password, options, &mut NoHooks)
}

fn encrypt_one_with(
    path: &Path,
    password: &str,
    options: &EncryptOptions,
    hooks: &mut dyn Hooks,
) -> Outcome {
    match run(path, password, options, hooks) {
        Ok(output) => Outcome::Encrypted { output },
        Err(Stop::Failed(error)) => Outcome::Failed { error },
        Err(Stop::Indeterminate { error, recovery }) => Outcome::Indeterminate {
            error,
            recovery_copy: recovery,
        },
    }
}

fn run(
    path: &Path,
    password: &str,
    options: &EncryptOptions,
    hooks: &mut dyn Hooks,
) -> std::result::Result<PathBuf, Stop> {
    validate_password(password).map_err(Stop::Failed)?;
    let meta = fs::symlink_metadata(path).map_err(|e| Stop::Failed(e.into()))?;
    if !meta.is_file() {
        return Err(Stop::Failed(Error::NotAFile(path.to_path_buf())));
    }
    if is_encrypted_file(path) {
        return Err(Stop::Failed(Error::AlreadyEncrypted));
    }
    let mut original = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| Stop::Failed(e.into()))?;
    let destination = encrypted_path(path);
    if is_taken(&destination) {
        return Err(Stop::Failed(Error::DestinationExists(destination)));
    }

    let temp = temp_path_for(path);
    let stats = stage(&mut original, &temp, password, options).map_err(Stop::Failed)?;

    if let Err(error) = verify(&temp, password, stats.plaintext_len) {
        let _ = fs::remove_file(&temp);
        return Err(Stop::Failed(error));
    }
    if let Err(error) = hooks.after_verify() {
        let _ = fs::remove_file(&temp);
        return Err(Stop::Failed(error.into()));
    }

    overwrite(&mut original, &temp, stats.ciphertext_len).map_err(|e| uncertain(e, &temp))?;
    hooks
        .after_overwrite()
        .map_err(|e| uncertain(e.into(), &temp))?;
    drop(original);

    match files_identical(path, &temp) {
        Ok(true) => {}
        Ok(false) => return Err(uncertain(Error::Corrupted, &temp)),
        Err(error) => return Err(uncertain(error, &temp)),
    }
    fs::rename(path, &destination).map_err(|e| uncertain(e.into(), &temp))?;
    if !is_encrypted_file(&destination) {
        return Err(uncertain(Error::Corrupted, &temp));
    }
    // The staging file only contains ciphertext; failing to remove it is harmless.
    let _ = fs::remove_file(&temp);
    Ok(destination)
}

/// Writes the ciphertext to a new staging file, removing it again on failure.
fn stage(
    original: &mut File,
    temp: &Path,
    password: &str,
    options: &EncryptOptions,
) -> Result<EncryptStats> {
    let file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    let result = stage_into(original, file, password, options);
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn stage_into(
    original: &mut File,
    file: File,
    password: &str,
    options: &EncryptOptions,
) -> Result<EncryptStats> {
    let mut writer = BufWriter::new(file);
    original.seek(SeekFrom::Start(0))?;
    let stats = encrypt_stream(original, &mut writer, password, options)?;
    let file = writer.into_inner().map_err(|e| Error::Io(e.into_error()))?;
    file.sync_all()?;
    Ok(stats)
}

fn verify(temp: &Path, password: &str, expected_plaintext_len: u64) -> Result<()> {
    let file = File::open(temp)?;
    let plaintext_len = decrypt_stream(file, &mut io::sink(), password)?;
    if plaintext_len == expected_plaintext_len {
        Ok(())
    } else {
        Err(Error::Corrupted)
    }
}

fn overwrite(original: &mut File, temp: &Path, expected_len: u64) -> Result<()> {
    let mut source = File::open(temp)?;
    original.seek(SeekFrom::Start(0))?;
    let copied = io::copy(&mut source, original)?;
    original.set_len(copied)?;
    original.sync_all()?;
    if copied == expected_len {
        Ok(())
    } else {
        Err(Error::Corrupted)
    }
}

fn files_identical(a: &Path, b: &Path) -> Result<bool> {
    let mut fa = File::open(a)?;
    let mut fb = File::open(b)?;
    let len = fa.metadata()?.len();
    if len != fb.metadata()?.len() {
        return Ok(false);
    }
    let mut buf_a = vec![0u8; IO_BLOCK_LEN];
    let mut buf_b = vec![0u8; IO_BLOCK_LEN];
    let mut remaining = len;
    while remaining > 0 {
        let n = usize::try_from(remaining.min(IO_BLOCK_LEN as u64)).unwrap_or(IO_BLOCK_LEN);
        fa.read_exact(&mut buf_a[..n])?;
        fb.read_exact(&mut buf_b[..n])?;
        if buf_a[..n] != buf_b[..n] {
            return Ok(false);
        }
        remaining -= n as u64;
    }
    Ok(true)
}

/// Encrypts every path with one password, reporting each file's outcome.
///
/// `progress(index, total, path)` is called before each file starts. A failure
/// of one file never aborts the batch; no file is skipped silently.
pub fn encrypt_batch(
    paths: &[PathBuf],
    password: &str,
    options: &EncryptOptions,
    progress: &mut dyn FnMut(usize, usize, &Path),
) -> Result<BatchReport> {
    if paths.is_empty() {
        return Err(Error::InvalidArgument("no files selected"));
    }
    validate_password(password)?;
    let total = paths.len();
    let mut report = BatchReport::default();
    for (index, path) in paths.iter().enumerate() {
        progress(index, total, path);
        let outcome = encrypt_one(path, password, options);
        report.files.push(FileReport {
            path: path.clone(),
            outcome,
        });
    }
    Ok(report)
}

/// Decrypts one encrypted file to `dest_path`.
///
/// Plaintext is written to a hidden staging file first; if any error occurs
/// (authentication failure, corruption, I/O), the staging file is overwritten
/// with zeros and removed so no unauthenticated or partial plaintext remains.
/// If `remove_source` is true, the source encrypted file is removed on success.
pub fn decrypt_one(
    encrypted_path: &Path,
    dest_path: &Path,
    password: &str,
    remove_source: bool,
) -> Outcome {
    let meta = match fs::symlink_metadata(encrypted_path) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed { error: e.into() },
    };
    if !meta.is_file() {
        return Outcome::Failed {
            error: Error::NotAFile(encrypted_path.to_path_buf()),
        };
    }
    if !is_encrypted_file(encrypted_path) {
        return Outcome::Failed {
            error: Error::NotEncrypted,
        };
    }
    if is_taken(dest_path) {
        return Outcome::Failed {
            error: Error::DestinationExists(dest_path.to_path_buf()),
        };
    }

    let temp = decrypted_temp_path_for(dest_path);
    let temp_file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
        Ok(f) => f,
        Err(e) => return Outcome::Failed { error: e.into() },
    };

    let decrypt_res = (|| -> Result<u64> {
        let mut writer = BufWriter::new(temp_file);
        let mut source_file = File::open(encrypted_path)?;
        let bytes_written = decrypt_stream(&mut source_file, &mut writer, password)?;
        let file = writer.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        file.sync_all()?;
        Ok(bytes_written)
    })();

    if let Err(error) = decrypt_res {
        let _ = shred::deep_delete(&temp);
        return Outcome::Failed { error };
    }

    if let Err(e) = fs::rename(&temp, dest_path) {
        let _ = shred::deep_delete(&temp);
        return Outcome::Failed { error: e.into() };
    }

    if remove_source {
        if let Err(e) = shred::normal_delete(encrypted_path) {
            return Outcome::Indeterminate {
                error: e,
                recovery_copy: None,
            };
        }
    }

    Outcome::Decrypted {
        output: dest_path.to_path_buf(),
    }
}

/// Decrypts every selected path using `password`.
///
/// Dest paths are determined by [`naming::decryption_destination`].
/// A failure of one file never stops the batch.
pub fn decrypt_batch(
    paths: &[PathBuf],
    password: &str,
    remove_source: bool,
    progress: &mut dyn FnMut(usize, usize, &Path),
) -> Result<BatchReport> {
    if paths.is_empty() {
        return Err(Error::InvalidArgument("no files selected"));
    }
    let total = paths.len();
    let mut report = BatchReport::default();
    for (index, path) in paths.iter().enumerate() {
        progress(index, total, path);
        let dest = match decryption_destination(path) {
            Destination::Restored(dest) => dest,
            Destination::NeedsChoice { suggested } => suggested,
        };
        let outcome = decrypt_one(path, &dest, password, remove_source);
        report.files.push(FileReport {
            path: path.clone(),
            outcome,
        });
    }
    Ok(report)
}

/// Updates the password of an encrypted file.
///
/// For Format v2 files, this performs an in-place atomic rewrite of the 129-byte header
/// without modifying or re-encrypting the content payload.
/// For Format v1 files, this performs a full re-encryption migration to Format v2
/// using the existing staging and overwrite protocol.
pub fn change_password_one(
    path: &Path,
    old_password: &str,
    new_password: &str,
    options: Option<&EncryptOptions>,
) -> Outcome {
    if let Err(e) = validate_password(new_password) {
        return Outcome::Failed { error: e };
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed { error: e.into() },
    };
    if !meta.is_file() {
        return Outcome::Failed {
            error: Error::NotAFile(path.to_path_buf()),
        };
    }
    if !is_encrypted_file(path) {
        return Outcome::Failed {
            error: Error::NotEncrypted,
        };
    }

    let header = match (|| -> Result<format::Header> {
        let mut file = File::open(path)?;
        format::read_header(&mut file)
    })() {
        Ok(h) => h,
        Err(e) => return Outcome::Failed { error: e },
    };

    if header.version == FORMAT_VERSION_V1 {
        migrate_v1_to_v2(path, old_password, new_password, options)
    } else {
        rewrap_file_v2(
            path,
            &header,
            old_password,
            new_password,
            options.map(|o| &o.kdf),
        )
    }
}

fn rewrap_file_v2(
    path: &Path,
    header: &format::Header,
    old_password: &str,
    new_password: &str,
    new_params: Option<&KdfParams>,
) -> Outcome {
    let new_header = match rewrap_header(header, old_password, new_password, new_params) {
        Ok(h) => h,
        Err(e) => return Outcome::Failed { error: e },
    };

    let backup_path = header_backup_path_for(path);
    if let Err(e) = (|| -> io::Result<()> {
        let mut backup = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup_path)?;
        backup.write_all(&header.to_bytes())?;
        backup.sync_all()
    })() {
        return Outcome::Failed { error: e.into() };
    }

    let write_res = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).open(path)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&new_header.to_bytes())?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = restore_header(path, &backup_path);
        let _ = fs::remove_file(&backup_path);
        return Outcome::Failed { error: e };
    }

    let verify_res = (|| -> Result<()> {
        let file = File::open(path)?;
        let mut decryptor = Decryptor::open(file, new_password)?;
        let mut buf = [0u8; 1];
        let _ = decryptor.read(&mut buf)?;
        Ok(())
    })();

    if let Err(e) = verify_res {
        let _ = restore_header(path, &backup_path);
        let _ = fs::remove_file(&backup_path);
        return Outcome::Failed { error: e };
    }

    let _ = fs::remove_file(&backup_path);
    Outcome::PasswordChanged {
        output: path.to_path_buf(),
    }
}

fn restore_header(target: &Path, backup: &Path) -> io::Result<()> {
    let old_bytes = fs::read(backup)?;
    let mut file = OpenOptions::new().write(true).open(target)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&old_bytes)?;
    file.sync_all()
}

fn migrate_v1_to_v2(
    path: &Path,
    old_password: &str,
    new_password: &str,
    options: Option<&EncryptOptions>,
) -> Outcome {
    let temp = temp_path_for(path);
    let default_opts = EncryptOptions::default();
    let opts = options.unwrap_or(&default_opts);

    let stage_res = (|| -> Result<EncryptStats> {
        let source_file = File::open(path)?;
        let mut decryptor = Decryptor::open(source_file, old_password)?;
        let mut dest_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let stats = encrypt_stream(&mut decryptor, &mut dest_file, new_password, opts)?;
        dest_file.sync_all()?;
        Ok(stats)
    })();

    let stats = match stage_res {
        Ok(s) => s,
        Err(e) => {
            let _ = fs::remove_file(&temp);
            return Outcome::Failed { error: e };
        }
    };

    if let Err(error) = verify(&temp, new_password, stats.plaintext_len) {
        let _ = fs::remove_file(&temp);
        return Outcome::Failed { error };
    }

    let mut original = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            let _ = fs::remove_file(&temp);
            return Outcome::Failed { error: e.into() };
        }
    };

    if let Err(e) = overwrite(&mut original, &temp, stats.ciphertext_len) {
        return Outcome::Indeterminate {
            error: e,
            recovery_copy: Some(temp),
        };
    }
    drop(original);

    match files_identical(path, &temp) {
        Ok(true) => {}
        Ok(false) => {
            return Outcome::Indeterminate {
                error: Error::Corrupted,
                recovery_copy: Some(temp),
            }
        }
        Err(e) => {
            return Outcome::Indeterminate {
                error: e,
                recovery_copy: Some(temp),
            }
        }
    }

    let _ = fs::remove_file(&temp);
    Outcome::PasswordChanged {
        output: path.to_path_buf(),
    }
}

/// Updates the password on every path in `paths`.
pub fn change_password_batch(
    paths: &[PathBuf],
    old_password: &str,
    new_password: &str,
    options: Option<&EncryptOptions>,
    progress: &mut dyn FnMut(usize, usize, &Path),
) -> Result<BatchReport> {
    if paths.is_empty() {
        return Err(Error::InvalidArgument("no files selected"));
    }
    validate_password(new_password)?;
    let total = paths.len();
    let mut report = BatchReport::default();
    for (index, path) in paths.iter().enumerate() {
        progress(index, total, path);
        let outcome = change_password_one(path, old_password, new_password, options);
        report.files.push(FileReport {
            path: path.clone(),
            outcome,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
