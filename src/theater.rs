//! Pure logic for theater mode and the boss key (no UI code).

use crate::constants::{
    BOSS_LEAVE_THEATER_TIMEOUT_FRAMES, BOSS_SETTLE_FRAMES, THEATER_HUD_TIMEOUT_SECS,
    THEATER_SWITCH_TIMEOUT_FRAMES,
};

/// How theater mode asks the window system to change the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TheaterStrategy {
    /// Ask for full screen and nothing else.
    Direct,
    /// Full screen, and hide and restore the window decorations explicitly.
    Decorations,
    /// Like `Decorations`, but a maximized window is first restored to its
    /// normal size and maximized again after theater mode ends.
    Unmaximize,
}

impl TheaterStrategy {
    /// Parses a setting such as `direct` (case-insensitive); `None` if unknown.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "direct" => Some(Self::Direct),
            "decorations" => Some(Self::Decorations),
            "unmaximize" => Some(Self::Unmaximize),
            _ => None,
        }
    }

    /// The strategy used when nothing is configured: on Linux some
    /// compositors (for example WSLg) leave the window frame visible over a
    /// full-screen window, so the more careful sequence is used there.
    pub fn platform_default() -> Self {
        if cfg!(target_os = "linux") {
            Self::Unmaximize
        } else {
            Self::Direct
        }
    }

    /// The configured strategy, or the platform default when the setting is
    /// missing or not recognised.
    pub fn from_setting(setting: Option<&str>) -> Self {
        setting
            .and_then(Self::parse)
            .unwrap_or_else(Self::platform_default)
    }

    fn hides_decorations(self) -> bool {
        self != Self::Direct
    }

    fn unmaximizes(self) -> bool {
        self == Self::Unmaximize
    }
}

/// A window change requested by [`WindowSequencer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCmd {
    /// Maximize (`true`) or restore (`false`) the window.
    Maximize(bool),
    /// Enter (`true`) or leave (`false`) full screen.
    Fullscreen(bool),
    /// Show (`true`) or hide (`false`) the window decorations.
    Decorations(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowStage {
    Idle,
    /// Waiting for a maximized window to be restored before full screen.
    Unmaximizing {
        frames: u16,
    },
    /// Waiting for full screen to end before maximizing again.
    Restoring {
        frames: u16,
    },
}

/// Orders the window changes of theater mode according to a
/// [`TheaterStrategy`], waiting for the window to report each step.
///
/// The caller feeds it the window's reported maximized and full-screen state
/// once per frame; `None` means the platform does not report it, in which case
/// a timeout ends the wait.
#[derive(Debug, Clone, Copy)]
pub struct WindowSequencer {
    strategy: TheaterStrategy,
    timeout: u16,
    stage: WindowStage,
    fullscreen_sent: bool,
    restore_maximized: bool,
}

impl WindowSequencer {
    /// Creates a sequencer that waits at most `timeout` frames for each step.
    pub fn new(strategy: TheaterStrategy, timeout: u16) -> Self {
        Self {
            strategy,
            timeout,
            stage: WindowStage::Idle,
            fullscreen_sent: false,
            restore_maximized: false,
        }
    }

    /// Whether full screen has been requested and not yet left again. The
    /// layout should follow this, not the user's intent, so that it does not
    /// change before the window does.
    pub fn fullscreen_requested(&self) -> bool {
        self.fullscreen_sent
    }

    /// Whether a multi-step change is still in progress.
    pub fn is_busy(&self) -> bool {
        self.stage != WindowStage::Idle
    }

    fn begin_fullscreen(&mut self) -> Vec<WindowCmd> {
        self.fullscreen_sent = true;
        let mut cmds = vec![WindowCmd::Fullscreen(true)];
        if self.strategy.hides_decorations() {
            cmds.push(WindowCmd::Decorations(false));
        }
        cmds
    }

    /// Theater mode was requested; `maximized` is the window's current state.
    pub fn enter(&mut self, maximized: bool) -> Vec<WindowCmd> {
        if self.fullscreen_sent || matches!(self.stage, WindowStage::Unmaximizing { .. }) {
            return Vec::new();
        }
        if matches!(self.stage, WindowStage::Restoring { .. }) {
            // Still full screen or just leaving it: go straight back in. The
            // window is to be maximized again when theater mode finally ends.
            self.stage = WindowStage::Idle;
            return self.begin_fullscreen();
        }
        if self.strategy.unmaximizes() && maximized {
            self.restore_maximized = true;
            self.stage = WindowStage::Unmaximizing { frames: 0 };
            return vec![WindowCmd::Maximize(false)];
        }
        self.restore_maximized = false;
        self.begin_fullscreen()
    }

    /// Theater mode ended (or was cancelled).
    pub fn leave(&mut self) -> Vec<WindowCmd> {
        if matches!(self.stage, WindowStage::Unmaximizing { .. }) {
            // Full screen was never requested; only undo the un-maximize.
            self.stage = WindowStage::Idle;
            let restore = std::mem::take(&mut self.restore_maximized);
            return if restore {
                vec![WindowCmd::Maximize(true)]
            } else {
                Vec::new()
            };
        }
        if !self.fullscreen_sent {
            return Vec::new();
        }
        self.fullscreen_sent = false;
        let mut cmds = vec![WindowCmd::Fullscreen(false)];
        if self.strategy.hides_decorations() {
            cmds.push(WindowCmd::Decorations(true));
        }
        if self.restore_maximized {
            self.stage = WindowStage::Restoring { frames: 0 };
        }
        cmds
    }

    /// Call once per frame with the window's reported state.
    pub fn advance(&mut self, maximized: Option<bool>, fullscreen: Option<bool>) -> Vec<WindowCmd> {
        match self.stage {
            WindowStage::Idle => Vec::new(),
            WindowStage::Unmaximizing { frames } => {
                if maximized == Some(false) || frames >= self.timeout {
                    self.stage = WindowStage::Idle;
                    self.begin_fullscreen()
                } else {
                    self.stage = WindowStage::Unmaximizing { frames: frames + 1 };
                    Vec::new()
                }
            }
            WindowStage::Restoring { frames } => {
                if fullscreen == Some(false) || frames >= self.timeout {
                    self.stage = WindowStage::Idle;
                    self.restore_maximized = false;
                    vec![WindowCmd::Maximize(true)]
                } else {
                    self.stage = WindowStage::Restoring { frames: frames + 1 };
                    Vec::new()
                }
            }
        }
    }
}

/// Which layout to draw this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutState {
    /// Side panel, toolbar and status bar are shown.
    Normal,
    /// Only the viewing pane (and the overlay) is shown.
    Theater,
    /// The window is changing size; draw nothing but the plain background.
    Switching,
}

/// Keeps the drawn layout in step with the window's real full-screen state.
///
/// The operating system needs a few frames to resize the window after a
/// full-screen request. Switching the layout immediately would draw the new
/// layout at the old window size for those frames, which shows up as
/// flicker. Instead the layout switches once the window reports the
/// requested state, or after a timeout when the platform never reports it.
#[derive(Debug, Clone, Copy)]
pub struct LayoutSync {
    shown_theater: bool,
    waiting: u16,
    timeout: u16,
}

impl Default for LayoutSync {
    fn default() -> Self {
        Self::new(THEATER_SWITCH_TIMEOUT_FRAMES)
    }
}

impl LayoutSync {
    /// Creates a synchronizer that waits at most `timeout` frames for the
    /// window; zero switches the layout immediately.
    pub fn new(timeout: u16) -> Self {
        Self {
            shown_theater: false,
            waiting: 0,
            timeout,
        }
    }

    fn settled(&self) -> LayoutState {
        if self.shown_theater {
            LayoutState::Theater
        } else {
            LayoutState::Normal
        }
    }

    /// Call once per frame. `requested` is whether theater mode is wanted and
    /// `reported` is the window's own full-screen state, if the platform gives one.
    pub fn update(&mut self, requested: bool, reported: Option<bool>) -> LayoutState {
        if requested == self.shown_theater {
            self.waiting = 0;
            return self.settled();
        }
        if reported == Some(requested) || self.waiting >= self.timeout {
            self.shown_theater = requested;
            self.waiting = 0;
            return self.settled();
        }
        self.waiting += 1;
        LayoutState::Switching
    }
}

/// Decides whether the theater overlay is visible, based on recent activity.
#[derive(Debug, Clone, Copy)]
pub struct HudTimer {
    last_activity: f64,
    timeout: f64,
}

impl Default for HudTimer {
    fn default() -> Self {
        Self::new(THEATER_HUD_TIMEOUT_SECS)
    }
}

impl HudTimer {
    /// Creates a timer that hides the overlay `timeout` seconds after the last activity.
    /// The overlay starts hidden.
    pub fn new(timeout: f64) -> Self {
        Self {
            last_activity: f64::NEG_INFINITY,
            timeout,
        }
    }

    /// Records user activity at time `now` (seconds).
    pub fn poke(&mut self, now: f64) {
        self.last_activity = now;
    }

    /// Hides the overlay immediately.
    pub fn hide(&mut self) {
        self.last_activity = f64::NEG_INFINITY;
    }

    /// Whether the overlay should be shown at time `now`.
    pub fn visible(&self, now: f64) -> bool {
        now - self.last_activity <= self.timeout
    }

    /// Seconds until the overlay hides (zero when already hidden).
    pub fn remaining(&self, now: f64) -> f64 {
        if self.visible(now) {
            (self.timeout - (now - self.last_activity)).max(0.0)
        } else {
            0.0
        }
    }
}

/// Where the boss-key sequence currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossStage {
    /// Nothing in progress.
    Idle,
    /// Locked; waiting for the window to leave full screen.
    LeavingTheater {
        /// Frames waited so far.
        frames: u16,
    },
    /// Locked and out of full screen; letting a cleared frame reach the screen.
    Settling {
        /// Frames still to wait before minimizing.
        frames_left: u8,
    },
}

/// What the caller should do after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossStep {
    /// Keep going; call [`BossSequence::advance`] again next frame.
    Wait,
    /// Send the minimize command now; the sequence is finished.
    Minimize,
}

/// Orders the boss-key actions so that nothing sensitive is on screen when the
/// window is minimized: lock first, leave full screen, wait a couple of frames
/// so a cleared frame is presented (operating systems snapshot the window for
/// thumbnails and animations), then minimize.
#[derive(Debug, Clone, Copy)]
pub struct BossSequence {
    stage: BossStage,
    settle_frames: u8,
    leave_timeout: u16,
}

impl Default for BossSequence {
    fn default() -> Self {
        Self::new(BOSS_SETTLE_FRAMES, BOSS_LEAVE_THEATER_TIMEOUT_FRAMES)
    }
}

impl BossSequence {
    /// Creates a sequence with explicit frame counts.
    pub fn new(settle_frames: u8, leave_timeout: u16) -> Self {
        Self {
            stage: BossStage::Idle,
            settle_frames,
            leave_timeout,
        }
    }

    /// The current stage.
    pub fn stage(&self) -> BossStage {
        self.stage
    }

    /// Whether a sequence is in progress.
    pub fn is_active(&self) -> bool {
        self.stage != BossStage::Idle
    }

    /// Starts the sequence. Returns `false` (doing nothing) if one is already
    /// running. The caller locks everything and, when `theater_active`, leaves
    /// theater mode right after this returns `true`.
    pub fn begin(&mut self, theater_active: bool) -> bool {
        if self.is_active() {
            return false;
        }
        self.stage = if theater_active {
            BossStage::LeavingTheater { frames: 0 }
        } else {
            BossStage::Settling {
                frames_left: self.settle_frames,
            }
        };
        true
    }

    /// Call once per frame, after the frame has been drawn. `fullscreen` is
    /// whether the window currently reports being full screen.
    pub fn advance(&mut self, fullscreen: bool) -> BossStep {
        match self.stage {
            BossStage::Idle => BossStep::Wait,
            BossStage::LeavingTheater { frames } => {
                self.stage = if !fullscreen || frames >= self.leave_timeout {
                    BossStage::Settling {
                        frames_left: self.settle_frames,
                    }
                } else {
                    BossStage::LeavingTheater { frames: frames + 1 }
                };
                BossStep::Wait
            }
            BossStage::Settling { frames_left } => {
                if frames_left == 0 {
                    self.stage = BossStage::Idle;
                    BossStep::Minimize
                } else {
                    self.stage = BossStage::Settling {
                        frames_left: frames_left - 1,
                    };
                    BossStep::Wait
                }
            }
        }
    }

    /// Abandons a running sequence.
    pub fn cancel(&mut self) {
        self.stage = BossStage::Idle;
    }
}

#[cfg(test)]
mod tests;
