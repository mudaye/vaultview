# VaultView user guide

VaultView views images and videos from your own folders and can encrypt files with a password. This guide explains each feature and, importantly, what the application **cannot** promise. Please read [Security limitations](#security-limitations) before you encrypt or delete anything you care about.

## Contents

- [VaultView user guide](#vaultview-user-guide)
  - [Contents](#contents)
  - [1. The window at a glance](#1-the-window-at-a-glance)
  - [2. Browsing files](#2-browsing-files)
  - [3. Moving between files](#3-moving-between-files)
  - [4. Viewing images and zooming](#4-viewing-images-and-zooming)
  - [5. Playing videos](#5-playing-videos)
  - [6. Favorites](#6-favorites)
  - [7. Encrypting files (batches)](#7-encrypting-files-batches)
  - [8. Opening encrypted files](#8-opening-encrypted-files)
  - [9. Password management](#9-password-management)
  - [10. Cache controls and closing media](#10-cache-controls-and-closing-media)
  - [11. Normal deletion](#11-normal-deletion)
  - [12. Deep deletion](#12-deep-deletion)
  - [Security limitations](#security-limitations)
  - [Troubleshooting](#troubleshooting)
  - [13. Keyboard and mouse reference](#13-keyboard-and-mouse-reference)

---

## 1. The window at a glance

- **Top bar:** the action buttons: *Encrypt…*, *Delete…*, *Deep delete…*, *Close media*, *Forget password*, *Clear decrypted cache*, *Lock all*, the image zoom buttons (shown only while an image is open), the previous/next arrows and *Help*.
- **Left panel:** two tabs, **Files** (the folder browser) and **Favorites**.
- **Center:** the viewer for the open image or video.
- **Bottom bar:** the open file's name, size in pixels, zoom level, position in the folder (for example `(3/12)`), a note when the file is encrypted, and short status messages that disappear after a few seconds.

In full screen (see sections 4 and 5) the top bar, left panel and bottom bar are hidden and only the viewer is shown.

## 2. Browsing files

**Moving around.** Use the drive buttons and *Home* at the top of the Files tab, click a folder to enter it, click *Up* to go to the parent folder, or type a path into the path box and press **Enter**. *Refresh* re-reads the folder. Tick *Hidden* to show hidden files.

**What the icons mean.**

| Icon | Meaning |
|---|---|
| 📁 | Folder |
| 🖼 | Image |
| 🎬 | Video |
| 🔒 | File encrypted by VaultView (shown in gold) |
| 📄 | Anything else |

VaultView identifies files by looking at their contents, not only their extension, so a photo with the wrong extension still shows as an image. Encrypted files are recognized by their internal signature, so a renamed encrypted file is still marked 🔒.

**Order.** Folders come first, then files, sorted by name with numbers compared as numbers (`img2` before `img10`) and ignoring upper/lower case.

**Opening a file.** Click its name. Your files are opened where they are; nothing is copied or imported. Files that are not supported, damaged or unreadable show a red message in the viewer instead of crashing the application, and the file is not changed.

**Selecting several files.** Tick the check boxes next to files (folders have no check box). Hold **Shift** while ticking to select everything from the last ticked file to this one. *Tick all files* and *Untick all* select or clear everything in the current folder. The count of ticked files is shown beside those buttons. Changing folders clears the selection.

The selection decides what *Encrypt…*, *Delete…* and *Deep delete…* act on (see sections 7, 11 and 12).

## 3. Moving between files

Press the **Left** and **Right** arrow keys, or click ◀ and ▶. They move through the images, videos and encrypted files of the current folder, in the same order as the list, skipping everything else.

- At the first or last file a message tells you there is nothing further.
- If you start with no file open, Right opens the first viewable file and Left opens the last.
- If an encrypted file cannot be opened (for example you cancel the password prompt or the password is wrong), VaultView does **not** skip ahead to the next file. The same neighbor is offered again on the next key press.
- The arrow keys only work while no text box has the keyboard focus and no dialog is open. If the path box has focus, click somewhere else first.

## 4. Viewing images and zooming

**Supported images:** JPEG, PNG, GIF, WebP, BMP and TIFF (first page only). Animated GIFs and animated WebP files play automatically and loop. A JPEG's rotation setting (EXIF orientation) is applied. AVIF works only if your copy of VaultView was built with the optional `avif` feature.

**Zoom and fit.**

| Action | Button | Key | Mouse |
|---|---|---|---|
| Zoom in / out | **+** / **−** | `+` or `=` / `-` | Mouse wheel (zooms at the pointer) |
| Actual size (100%) | **100%** | `1` | Double-click toggles 100% and fit |
| Fit to window | **Fit window** | | |
| Fit image (never enlarges small images) | **Fit image** | `2` | |
| Reset zoom and position | **Reset** | `0` | |
| Pan | | `W` `A` `S` `D` | Drag with the mouse |

When you open an image it starts in *Fit window*. Panning only works when the zoomed image is larger than the window; you cannot drag it off the screen. Zoom is limited to between 5% and 3200%.

**Full screen:** press `F` or `F11`. Leave with `Esc` (or `F`/`F11` again).

**Large images.** Images wider or taller than 8192 pixels are shrunk for display and the bottom bar says so; zooming in will not reveal more detail than the shrunken copy. Images with more than 200 million pixels are refused with an error message.

## 5. Playing videos

Video playback requires GStreamer on your computer. Which formats, audio tracks, subtitles and special features (such as HDR) work depends on the GStreamer plugins installed. If a video does not play, the viewer shows an error. Builds made without video support say so when you click a video.

**Controls under the picture.**

| Control | What it does |
|---|---|
| Play / Pause | Starts or pauses. Starting again after the end restarts from the beginning |
| Seek bar and time | Drag to jump; the time shows `position / total` |
| Volume slider and **Mute** | Volume from 0 to 100%, and mute |
| **Audio** menu | Appears only when the video has more than one audio track |
| **Subtitles** menu | Appears only when the video has subtitle tracks; choose a track or *Off* |
| **Fullscreen** | Full-screen playback |

**Keys and mouse:** `Space` or `K` play/pause · `J` / `L` jump back / forward 5 seconds · `Up` / `Down` volume · `M` mute · `F` full screen. Click the picture to play/pause and double-click it for full screen.

**Notes.**

- Seeking jumps to a nearby key frame, so it may land slightly before or after the exact spot you chose.
- Videos start playing as soon as they open.
- Opening another file, or closing the media, stops the video and releases its resources.

## 6. Favorites

Favorites are shortcuts to folders you use often.

- **Add:** open the folder in the Files tab and click **★ Favorite**.
- **Use:** switch to the **Favorites** tab and click a folder to open it in the browser.
- **Remove:** click **Remove** next to it. This only removes the shortcut; the folder and its files are untouched.

A favorite whose folder has been moved, deleted, turned into a file or become inaccessible (for example an unplugged drive) is shown with a warning (*missing*, *not a folder* or *inaccessible*) and cannot be opened. The status is re-checked every couple of seconds while the tab is visible, or when you click *Re-check*. Favorites are kept between sessions and contain only folder locations, never file contents or passwords. They are stored in your user configuration folder (see the README for the path).

## 7. Encrypting files (batches)

Encryption replaces each file's contents, in place, with an encrypted version, and adds `.vvenc` to its name: `holiday.jpg` becomes `holiday.jpg.vvenc`. Encryption and decryption need no internet connection or account.

**Step by step.**

1. Tick the files in the Files tab. (If nothing is ticked, VaultView uses the file you currently have open, if it is not already encrypted.)
2. Click **Encrypt…**. Folders and files that are already encrypted are ignored.
3. In the dialog, type the password twice. It must have at least 8 characters. Tick *Show password* if you want to see what you typed.
4. Tick *I understand that a lost password means permanent data loss*. Nothing can be encrypted until you do.
5. Optionally untick *Compress before encrypting when it helps* (it only compresses data that compresses well, so photos and videos are normally stored as they are).
6. Click **Encrypt**. A progress window shows the current file. Wait for it to finish; the open media is closed first.

**One password per batch.** All files in one batch share the password you type, but each file also gets its own random key, so each file can be opened on its own without the others. The password is not remembered for future batches: the next time you encrypt, you choose a new password (you may of course choose the same one).

**The result report.** When the batch finishes you get a list with one line per file:

| Result | Meaning |
|---|---|
| **Encrypted** (green) | The file was encrypted, checked and renamed. |
| **Failed** (red) | The file was **not changed**. The reason is given (for example a file already encrypted, an existing `.vvenc` file with the same name, no write permission). |
| **UNCERTAIN** (orange) | The file's contents may have been replaced but VaultView could not confirm that everything finished. A complete, checked encrypted copy was kept; its name is in the message. See [Troubleshooting](#troubleshooting). |

A failure in one file does not stop or undo the others. VaultView never silently skips a selected file.

**How the safety check works.** Before touching your original, VaultView writes the encrypted data to a hidden helper file next to it (`.<name>.vvtmp`) and checks that it decrypts correctly with your password. Only then is your original overwritten and renamed. On success the helper file is removed.

**Important.** Encrypting a file does not erase earlier copies of it: backups, cloud-synced copies, old versions, and traces left in the storage device may still hold the original. See [Security limitations](#security-limitations).

**Passwords and time.** Each file takes a moment because the password is deliberately slow to process (to make guessing harder), and the result is checked by decrypting it once more.

## 8. Opening encrypted files

Click a 🔒 file (or arrive at one with the arrow keys).

- If a password you already used in this session fits, the file opens immediately.
- Otherwise a **Password required** window appears. Type the password and press **Enter** or click **Open**.
- A wrong password shows a message and asks again. Cancel to give up. In every failure case the encrypted file itself is not modified.
- The viewer is blank while the password window is open.

Images are decrypted into memory and shown. Videos are decrypted in small pieces while they play, and seeking works, so no permanent plain copy is created. The bottom bar marks encrypted files that are being viewed.

**Messages you may see.**

| Message | Meaning |
|---|---|
| Wrong password, or the file's key header was modified | The password is wrong, or the first part of the file was altered. VaultView cannot tell which. |
| The encrypted file is corrupted, truncated or was tampered with | The file is damaged, cut short, extended or modified. Nothing from it is shown. |
| Unsupported encrypted-file format version | The file was made by a newer version of the format. |
| Not a supported image or video | The file decrypted correctly but the contents are not an image or video this application can show. |
| Too large | The content exceeds the size limits (see the README). |

**Limits for encrypted media.**

- Encrypted images are limited to 256 MiB after decryption/decompression.
- A video that was stored compressed inside its encrypted file cannot be streamed; it is decrypted completely into memory first (same 256 MiB limit). Videos encrypted by this version are normally not stored compressed.
- Passwords are case-sensitive and compared exactly as typed. A different keyboard layout or input method can produce different characters.

**There is currently no "decrypt to file" feature.** VaultView can show encrypted media but cannot write a permanent decrypted copy back to disk. The encrypted-file format is documented in `docs/FORMAT.md` for anyone who needs to build a decryption tool, but a lost password still means lost data.

## 9. Password management

- **Where passwords live.** Passwords are only kept in memory during the current session. They are never written to a file, a log, the favorites list or any setting.
- **What is retained.** Passwords that successfully opened a file, or that you used to encrypt a batch, are remembered for the session (the last 8) so you are not asked again for files they fit. Opening an encrypted file tries them automatically.
- **Forget Password** (top bar) removes every remembered password. After that you are asked again when you open an encrypted file that is not in the cache (see the next section).
- **Quitting** the application normally forgets all passwords.
- **Typing.** Password fields hide what you type by default. The encrypt dialog has a *Show password* option.
- **If you lose a password,** the files it protects cannot be recovered by anyone, including the authors of this application. There is no reset, no backup key and no online service. Keep your passwords somewhere safe, for example a password manager.

## 10. Cache controls and closing media

Decrypted *images* are kept in an in-memory cache (up to 256 MiB in total) so going back and forth between them is fast. Nothing in the cache is written to disk, and the cache is emptied when the application exits normally. Decrypted videos are not cached.

| Button | Effect |
|---|---|
| **Close media** | Closes the open image or video and releases its picture, its video player and its buffers. |
| **Clear decrypted cache** | Empties the in-memory cache of decrypted images. An image that is currently displayed stays on screen until you close it or open something else. |
| **Forget password** | Forgets all remembered passwords (section 9). |
| **Lock all** | Does all three: closes the media, forgets all passwords and clears the cache. |

**Be aware:** an image that is still in the cache opens again without asking for a password, even after *Forget password*. If you want a file to require its password again, use **Lock all** (or *Clear decrypted cache* together with *Forget password*).

## 11. Normal deletion

1. Tick the files to delete. **If you have ticked files, only those are deleted, not the file you have open.** If nothing is ticked, the open file is deleted.
2. Click **Delete…** and confirm in the dialog.

Normal deletion uses the operating system's standard file deletion. It does **not** use the Recycle Bin or Trash, so you cannot restore the file from there. The data may remain recoverable with special tools until the storage space is reused.

Only files can be deleted, not folders. Files in the selection that are encrypted are deleted too. A report lists each file; failures are shown and the file is left as it was.

## 12. Deep deletion

Deep deletion works on ordinary and encrypted files. It overwrites the whole file **three times with random data**, forces each pass to be written out, and then requests normal deletion.

1. Tick the files (or open one, if none is ticked; the same rule as in section 11 applies).
2. Click **Deep delete…**.
3. Read the warning, tick *I understand that physical erasure cannot be guaranteed*, and click **Deep delete**.

The report states how many passes finished. A file is only reported as deep-deleted if every pass and the final deletion succeeded. If something fails, the message says the file may still exist and may have been partly overwritten. Symbolic links and folders are refused, so a shortcut can never cause its target to be shredded.

**What deep deletion does not do.** Overwriting a file does not guarantee that its data is gone from the storage device. See the next section.

---

## Security limitations

Please understand these before relying on VaultView for sensitive data.

**Password loss is permanent.** There is no recovery mechanism, backdoor or online service. If you forget the password, the encrypted files are lost.

**Password strength matters.** Anyone who gets an encrypted file can try to guess the password offline. The key derivation makes each guess slow but cannot protect a short or common password. Use a long, unique password.

**Encryption does not remove earlier copies.** If a file existed in plain form before you encrypted it, copies may remain in backups, cloud-sync folders, "previous versions"/snapshots, the Recycle Bin or Trash, thumbnail caches made by other programs, e-mail attachments and on the storage device itself. VaultView overwrites the original file's contents when it encrypts, but see the next point.

**Overwriting cannot guarantee physical erasure.** This applies both to in-place encryption and to deep deletion. On SSDs and other wear-levelled storage, flash drives and memory cards, journaling filesystems, copy-on-write filesystems, snapshots, backups and any device whose physical writes the application does not control, the old data can survive somewhere else on the device. Three overwrite passes do **not** change this. Deep-deleting an encrypted file does not remove earlier plain copies of that file either. For serious needs, use full-disk encryption from the start, and destroy or securely erase the whole device when disposing of it.

**Decrypted content and your computer's memory.** VaultView does not intentionally write decrypted media to disk: no plain temporary files, previews or thumbnails are made. It clears its own memory buffers and caches when it releases them. It cannot control, and cannot promise anything about: the operating system's paging, swap file or hibernation file; crash dumps and diagnostic tools; graphics drivers and video memory; hardware video decoders; GStreamer's internal buffers; or the text-field history kept by the user-interface toolkit. Decrypted data can therefore reach persistent storage by these routes. Clearing the cache and quitting the application reduce, but do not eliminate, this exposure.

**Names and sizes are visible.** The `.vvenc` extension is added to the original name, so file names are not hidden. File sizes remain roughly visible. Rename files first if the name itself is sensitive.

**Other threats are out of scope.** VaultView does not protect against malware or keyloggers on your computer, screen capture, someone looking over your shoulder, or a person with access to your running, unlocked session. Use *Lock all* when you step away.

**No independent review.** The encryption design uses well-known algorithms (XChaCha20-Poly1305 and Argon2id), but this implementation has not been independently audited.

## Troubleshooting

**An encryption result says UNCERTAIN.** Something failed after VaultView started overwriting the original. A complete, verified encrypted copy was kept as a hidden file named `.<name>.vvtmp` next to the original (turn on *Hidden* in the Files tab to see it); the message gives its name. Suggested recovery (reasoned, not tested):

1. Open the recovery file with your password to confirm it shows the right content.
2. If the file at the original name is also 🔒, it already holds the encrypted data; rename it to add `.vvenc` if it lacks the extension.
3. Otherwise rename the recovery file to `<original name>.vvenc`.
4. Delete leftover copies only after you have confirmed the encrypted file opens.

**A file failed with "destination already exists".** A file named `<name>.vvenc` is already in the folder. Rename or move one of them and try again. VaultView never overwrites an existing file.

**A file failed with "already encrypted".** The file already carries the encrypted-file signature, whatever its name.

**A video will not play.** GStreamer may be missing or lack the plugin for that codec. Install the GStreamer "good", "bad", "ugly" and "libav" plugin sets (see the README), then restart VaultView. If the application was built without video support, videos cannot be played at all.

**An image is blurry when zoomed in.** It was larger than 8192 pixels and was shrunk for display (the bottom bar says so).

**The arrow keys do nothing.** A text box (such as the path box) may have the keyboard focus, or a dialog or a running operation is open. Click an empty part of the viewer and try again.

**The Favorites list started empty after an error message.** The favorites file could not be read (for example it was damaged). The old file stays untouched until you add or remove a favorite, which saves a new list.

**Something else went wrong.** The message in the viewer or in the result report names the affected file and the reason. It is safe to retry: operations never overwrite existing files silently, and failed decryption never modifies the encrypted file.

## 13. Keyboard and mouse reference

Keyboard shortcuts work when no text box has the focus and no dialog is open.

| Key | Action |
|---|---|
| `Left` / `Right` | Previous / next file |
| `+` or `=` / `-` | Zoom in / out |
| `1` | Actual size (100%) |
| `2` | Fit image (no enlarging) |
| `0` | Reset zoom and position (fit to window) |
| `W` `A` `S` `D` | Pan a zoomed image |
| `F` or `F11` | Toggle full screen |
| `Esc` | Leave full screen |
| `Space` or `K` | Video: play / pause |
| `J` / `L` | Video: back / forward 5 seconds |
| `Up` / `Down` | Video: volume up / down |
| `M` | Video: mute |

| Mouse | Action |
|---|---|
| Click a folder / file | Open it |
| Click a check box | Tick / untick a file |
| `Shift` + click a check box | Tick a range of files |
| Mouse wheel over an image | Zoom at the pointer |
| Drag an image | Pan |
| Double-click an image | Toggle 100% and fit |
| Click a video | Play / pause |
| Double-click a video | Full screen |