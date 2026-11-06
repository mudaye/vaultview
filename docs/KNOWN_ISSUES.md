# Known issues

The project is paused. This file records everything that is known to be wrong or
unverified, so work can resume without rediscovering it. Items marked **Bug**
are defects; **Platform** items are limits of an environment; **Gap** items are
missing features; **Unverified** items have simply never been checked.

Environments used so far: Windows 10 and Ubuntu 22.04 (apparently under WSL2/WSLg:
Wayland, window frames drawn by the toolkit, no X11 installed). macOS has never
been tried.

## Theater mode and the window

### KI-1 (Bug) Linux/WSLg: a window frame outline stays visible in theater mode when the window was maximized first

- **Symptom:** in theater mode (borderless full screen) the old window frame
  (title bar and/or a thin border at the previous window geometry) remains drawn
  over the content. Without maximizing first it was fixed by hiding the window
  decorations explicitly; with the window maximized the outline remains.
- **Tried, did not fix it (as reported on Ubuntu/WSLg):**
  1. Hiding and restoring the decorations around the full-screen request
     (`VAULTVIEW_THEATER=decorations`). Fixed the non-maximized case only.
  2. Restoring a maximized window first, going full screen, and maximizing
     again afterwards (`VAULTVIEW_THEATER=unmaximize`, the Linux default). No
     visible improvement.
- **What is known:** the application draws only inside its own window; the
  leftover frame looks like decoration drawn by the toolkit or compositor at
  the old geometry. WSLg's compositor is a known source of window-state
  problems (for example protocol errors after maximizing).
- **Not known:** whether it also happens on native Linux (Wayland or X11); the
  X11 backend could not be tried (not installed).
- **Workaround:** use `VAULTVIEW_THEATER=direct|decorations|unmaximize` to
  compare behaviors; avoid theater mode with a maximized window under WSLg.
- **Ideas not yet tried:** a newer eframe/winit (0.32 or later has Wayland
  sizing fixes, but the UI code would need porting); testing on native Linux;
  installing the X11 libraries and forcing X11; reporting to winit/WSLg.
- **Code:** `src/theater.rs` (`TheaterStrategy`, `WindowSequencer`),
  `set_theater_window` and the `update` loop in `src/app.rs`.

### KI-2 (Bug) Windows: theater-mode flicker reported again after the last change

- **History:** the original immediate layout switch flickered on every toggle.
  Switching the layout only after the window reports its new full-screen state
  (`LayoutSync`, `THEATER_SWITCH_TIMEOUT_FRAMES`) was reported as fixing it.
  The following build (decorations hidden on Linux only) still passed manual
  tests on Windows and Ubuntu. After the build that added `WindowSequencer`, the
  flicker was reported back on Windows.
- **Not understood:** on Windows the default strategy is `direct`, which sends
  exactly the same window commands as the previous build, so the code path
  should be unchanged. Possible explanations: an unintended difference in the
  frame timing of the layout switch; a flicker that is intermittent and was
  never fully fixed; something else in that build.
- **First thing to do when resuming:** confirm it (several toggles, in both
  directions, with and without a video or PDF open), then bisect by reverting
  `WindowSequencer` and comparing; also try `THEATER_SWITCH_TIMEOUT_FRAMES = 0`
  (restores the very old immediate switch) to see what changes.
- **Code:** same as KI-1, plus `LayoutSync`.

### KI-3 (Bug, not reproducing) Ubuntu/WSLg: crash after maximize, minimize, maximize

- **Symptom:** the application exited after the sequence maximize, minimize,
  maximize, every time. Console: `Io error: Broken pipe (os error 32)` three
  times, then `Error: WinitEventLoop(ExitFailure(1))`: the Wayland connection to
  the compositor was dropped.
- **Status:** stopped happening after the layout-synchronization change, but no
  code change explains why, so it is not considered fixed. A matching WSLg bug
  exists upstream (apps killed after maximizing, "Protocol error").
- **If it returns:** run with `WAYLAND_DEBUG=1` and look at the last messages
  before the crash.

### KI-4 (Platform) WSLg full-screen window smaller than the display

Under WSLg a Linux window was limited to 1920 px wide on a 2560 px screen
(centered). Not controllable by the application.

### KI-5 (By design, trade-off) Esc is always the boss key

Esc locks everything, closes dialogs, leaves theater mode and minimizes, in
every state of the main window, also while a text box has focus. Pressing Esc to
cancel an edit therefore also locks. Esc no longer leaves theater mode (use F,
F11 or the overlay's Exit button). There is no system-wide hotkey: the boss key
works only while the application window has the keyboard focus.

## PDF

### KI-6 (Limitation) Pure-Rust PDF renderer (hayro 0.6)

- PDFs protected by their own password are not opened. JPEG 2000 images, some
  fonts and scripts, and complex transparency may be skipped or misrendered;
  the page shows a notice when the renderer reports skipped content.
- Pages are rendered whole. At very high zoom a large page is rendered at
  reduced resolution (24 million pixel and 8192 px per side limits) and looks
  soft. A fresh render cache is created for every render (hayro ties the cache
  to the page), which costs speed on complex pages.
- No text selection or search, links, annotations or forms. "100%" means the
  page's nominal size (72 points per inch). Files over 256 MiB are refused.
- Encrypted PDFs are decrypted into memory and handed to the PDF library, which
  keeps its own copy; that copy cannot be zeroized.
- Bundled "Foxit" PDF fonts (hayro `embed-fonts`) are under the PDFium
  BSD-style license: binary distributions must reproduce its notice (see
  ROADMAP, release hygiene).
- Needs Rust 1.92 or newer because of hayro's dependencies.

## Audio and video

### KI-7 (Unverified) Audio on real files

Tag and cover-art extraction is unit-tested with synthetic tag lists only.
Playback, tags and cover art on real MP3/FLAC/Ogg/M4A files, audio-only
MKV/MP4 detection, and the 🎵/📕 icons in the file list have not been checked
by anyone. Audio and video need GStreamer and the default `video` feature.

### KI-8 (Limitation) Compressed encrypted video is decrypted fully into memory

An encrypted video stored compressed (rare) cannot be streamed; it is
decrypted into memory (256 MiB limit). Seeking snaps to key frames. Codec,
subtitle, HDR and rotation support depend on the installed GStreamer plugins.

## General

### KI-9 (Limitation) Argon2 runs on the interface thread when opening encrypted files

About 64 MiB of memory and a noticeable fraction of a second; the window can
pause briefly. Encryption and deletion already run on a worker thread.

### KI-10 (Limitation) Images

Images larger than 8192 px per side are downscaled for display (no tiled
rendering); images and animations over 200 million pixels are refused.

### KI-11 (Gap) Missing features

Changing the password of an encrypted file and decrypting a file back to disk are
not implemented (see ROADMAP). Encrypted files can only be viewed.

### KI-12 (Security caveats) Residual exposure the application cannot remove

Decrypted data can reach disk through operating-system paging, swap and
hibernation, crash dumps, graphics and decoder buffers and GStreamer internals;
the GUI toolkit may keep typed text in its edit history; the operating system may
keep window thumbnails or previews, so the boss key reduces but cannot guarantee
what is retained; overwriting does not guarantee physical erasure on SSDs,
journaling or copy-on-write storage, snapshots or backups. No independent
security review of the format or code has been done, and there are no published
known-answer test vectors for the file format.

### KI-13 (Unverified) Platforms

Never run on macOS or on a Linux desktop outside WSL. Windows 10 and Ubuntu on
WSLg are the only tested environments, and only the unit tests plus manual
checks reported in the project history.
