#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::{
    BOSS_LEAVE_THEATER_TIMEOUT_FRAMES, BOSS_SETTLE_FRAMES, THEATER_HUD_TIMEOUT_SECS,
    THEATER_SWITCH_TIMEOUT_FRAMES,
};

// ---------- overlay timer ----------

#[test]
fn the_overlay_starts_hidden() {
    let timer = HudTimer::new(3.0);
    assert!(!timer.visible(0.0));
    assert!(!timer.visible(1_000.0));
    assert_eq!(timer.remaining(0.0), 0.0);
}

#[test]
fn activity_shows_the_overlay_until_the_timeout() {
    let mut timer = HudTimer::new(3.0);
    timer.poke(10.0);
    assert!(timer.visible(10.0));
    assert!(timer.visible(12.9));
    assert!(timer.visible(13.0));
    assert!(!timer.visible(13.1));
    assert!((timer.remaining(11.0) - 2.0).abs() < 1e-9);
    assert_eq!(timer.remaining(20.0), 0.0);
}

#[test]
fn new_activity_extends_the_visible_period() {
    let mut timer = HudTimer::new(3.0);
    timer.poke(10.0);
    timer.poke(12.5);
    assert!(timer.visible(15.0));
    assert!(!timer.visible(15.6));
}

#[test]
fn hiding_takes_effect_immediately() {
    let mut timer = HudTimer::new(3.0);
    timer.poke(10.0);
    timer.hide();
    assert!(!timer.visible(10.0));
}

#[test]
fn the_default_timer_uses_the_configured_timeout() {
    let mut timer = HudTimer::default();
    timer.poke(0.0);
    assert!(timer.visible(THEATER_HUD_TIMEOUT_SECS));
    assert!(!timer.visible(THEATER_HUD_TIMEOUT_SECS + 0.01));
}

// ---------- boss-key sequence ----------

#[test]
fn a_new_sequence_is_idle_and_ignores_advances() {
    let mut seq = BossSequence::new(2, 90);
    assert!(!seq.is_active());
    assert_eq!(seq.stage(), BossStage::Idle);
    assert_eq!(seq.advance(false), BossStep::Wait);
    assert_eq!(seq.advance(true), BossStep::Wait);
    assert!(!seq.is_active());
}

#[test]
fn outside_theater_mode_it_waits_the_settle_frames_then_minimizes_once() {
    let mut seq = BossSequence::new(2, 90);
    assert!(seq.begin(false));
    assert_eq!(seq.stage(), BossStage::Settling { frames_left: 2 });
    assert_eq!(seq.advance(false), BossStep::Wait);
    assert_eq!(seq.advance(false), BossStep::Wait);
    assert!(seq.is_active());
    assert_eq!(seq.advance(false), BossStep::Minimize);
    assert!(!seq.is_active());
    assert_eq!(
        seq.advance(false),
        BossStep::Wait,
        "minimize is only requested once"
    );
}

#[test]
fn it_never_minimizes_in_the_frame_that_started_it() {
    let mut seq = BossSequence::new(0, 90);
    assert!(seq.begin(false));
    // begin() runs while the frame is built; advance() runs at its end.
    assert_eq!(seq.advance(false), BossStep::Minimize);
    let mut with_settle = BossSequence::new(1, 90);
    with_settle.begin(false);
    assert_eq!(with_settle.advance(false), BossStep::Wait);
}

#[test]
fn a_second_trigger_while_active_is_ignored() {
    let mut seq = BossSequence::new(2, 90);
    assert!(seq.begin(false));
    assert!(!seq.begin(false));
    assert!(!seq.begin(true));
    assert_eq!(seq.stage(), BossStage::Settling { frames_left: 2 });
}

#[test]
fn in_theater_mode_it_waits_for_the_window_to_leave_full_screen() {
    let mut seq = BossSequence::new(2, 90);
    assert!(seq.begin(true));
    assert_eq!(seq.stage(), BossStage::LeavingTheater { frames: 0 });
    assert_eq!(seq.advance(true), BossStep::Wait);
    assert_eq!(seq.advance(true), BossStep::Wait);
    assert_eq!(seq.stage(), BossStage::LeavingTheater { frames: 2 });
    assert_eq!(seq.advance(false), BossStep::Wait);
    assert_eq!(seq.stage(), BossStage::Settling { frames_left: 2 });
    assert_eq!(seq.advance(false), BossStep::Wait);
    assert_eq!(seq.advance(false), BossStep::Wait);
    assert_eq!(seq.advance(false), BossStep::Minimize);
    assert!(!seq.is_active());
}

#[test]
fn it_does_not_wait_forever_for_full_screen_to_end() {
    let mut seq = BossSequence::new(1, 2);
    assert!(seq.begin(true));
    let mut steps = 0;
    let result = loop {
        steps += 1;
        assert!(steps < 20, "sequence never finished");
        if seq.advance(true) == BossStep::Minimize {
            break steps;
        }
    };
    assert_eq!(result, 5);
    assert!(!seq.is_active());
}

#[test]
fn cancel_abandons_a_running_sequence() {
    let mut seq = BossSequence::new(2, 90);
    seq.begin(true);
    seq.cancel();
    assert!(!seq.is_active());
    assert!(seq.begin(false), "a new sequence can start after cancel");
}

#[test]
fn the_default_sequence_uses_the_configured_frame_counts() {
    let mut seq = BossSequence::default();
    assert!(seq.begin(false));
    assert_eq!(
        seq.stage(),
        BossStage::Settling {
            frames_left: BOSS_SETTLE_FRAMES
        }
    );
    let mut theater = BossSequence::default();
    theater.begin(true);
    for _ in 0..BOSS_LEAVE_THEATER_TIMEOUT_FRAMES {
        theater.advance(true);
    }
    assert_eq!(
        theater.stage(),
        BossStage::LeavingTheater {
            frames: BOSS_LEAVE_THEATER_TIMEOUT_FRAMES
        }
    );
    theater.advance(true);
    assert_eq!(
        theater.stage(),
        BossStage::Settling {
            frames_left: BOSS_SETTLE_FRAMES
        }
    );
}

// ---------- layout synchronisation ----------

#[test]
fn a_stable_layout_never_switches() {
    let mut sync = LayoutSync::new(5);
    for _ in 0..10 {
        assert_eq!(sync.update(false, Some(false)), LayoutState::Normal);
        assert_eq!(sync.update(false, None), LayoutState::Normal);
    }
}

#[test]
fn entering_theater_mode_waits_for_the_window_to_report_full_screen() {
    let mut sync = LayoutSync::new(30);
    assert_eq!(sync.update(true, Some(false)), LayoutState::Switching);
    assert_eq!(sync.update(true, Some(false)), LayoutState::Switching);
    assert_eq!(sync.update(true, Some(true)), LayoutState::Theater);
    assert_eq!(sync.update(true, Some(true)), LayoutState::Theater);
}

#[test]
fn leaving_theater_mode_waits_for_the_window_to_leave_full_screen() {
    let mut sync = LayoutSync::new(30);
    sync.update(true, Some(true));
    assert_eq!(sync.update(false, Some(true)), LayoutState::Switching);
    assert_eq!(sync.update(false, Some(true)), LayoutState::Switching);
    assert_eq!(sync.update(false, Some(false)), LayoutState::Normal);
}

#[test]
fn a_window_that_already_reports_the_new_state_causes_no_switching_frame() {
    let mut sync = LayoutSync::new(30);
    assert_eq!(sync.update(true, Some(true)), LayoutState::Theater);
    assert_eq!(sync.update(false, Some(false)), LayoutState::Normal);
}

#[test]
fn a_platform_that_never_reports_the_state_is_handled_by_the_timeout() {
    let mut sync = LayoutSync::new(3);
    for frame in 0..3 {
        assert_eq!(
            sync.update(true, None),
            LayoutState::Switching,
            "frame {frame}"
        );
    }
    assert_eq!(sync.update(true, None), LayoutState::Theater);
    for _ in 0..3 {
        assert_eq!(sync.update(false, None), LayoutState::Switching);
    }
    assert_eq!(sync.update(false, None), LayoutState::Normal);
}

#[test]
fn a_wrong_report_does_not_confirm_the_change() {
    let mut sync = LayoutSync::new(30);
    for _ in 0..10 {
        assert_eq!(sync.update(true, Some(false)), LayoutState::Switching);
    }
}

#[test]
fn a_zero_timeout_switches_immediately_like_the_old_behaviour() {
    let mut sync = LayoutSync::new(0);
    assert_eq!(sync.update(true, Some(false)), LayoutState::Theater);
    assert_eq!(sync.update(false, Some(true)), LayoutState::Normal);
    assert_eq!(sync.update(true, None), LayoutState::Theater);
}

#[test]
fn reversing_the_request_while_switching_starts_over() {
    let mut sync = LayoutSync::new(3);
    assert_eq!(sync.update(true, Some(false)), LayoutState::Switching);
    assert_eq!(sync.update(true, Some(false)), LayoutState::Switching);
    assert_eq!(sync.update(false, Some(false)), LayoutState::Normal);
    // a later request gets its full waiting time again
    for _ in 0..3 {
        assert_eq!(sync.update(true, None), LayoutState::Switching);
    }
    assert_eq!(sync.update(true, None), LayoutState::Theater);
}

#[test]
fn the_default_uses_the_configured_timeout() {
    let mut sync = LayoutSync::default();
    for _ in 0..THEATER_SWITCH_TIMEOUT_FRAMES {
        assert_eq!(sync.update(true, None), LayoutState::Switching);
    }
    assert_eq!(sync.update(true, None), LayoutState::Theater);
}

// ---------- theater strategy and window sequencing ----------

use WindowCmd::{Decorations, Fullscreen, Maximize};

#[test]
fn strategies_parse_case_insensitively_and_reject_unknown_values() {
    assert_eq!(
        TheaterStrategy::parse("direct"),
        Some(TheaterStrategy::Direct)
    );
    assert_eq!(
        TheaterStrategy::parse(" Decorations "),
        Some(TheaterStrategy::Decorations)
    );
    assert_eq!(
        TheaterStrategy::parse("UNMAXIMIZE"),
        Some(TheaterStrategy::Unmaximize)
    );
    assert_eq!(TheaterStrategy::parse(""), None);
    assert_eq!(TheaterStrategy::parse("fullscreen"), None);
}

#[test]
fn the_platform_default_is_careful_only_on_linux() {
    let expected = if cfg!(target_os = "linux") {
        TheaterStrategy::Unmaximize
    } else {
        TheaterStrategy::Direct
    };
    assert_eq!(TheaterStrategy::platform_default(), expected);
    assert_eq!(TheaterStrategy::from_setting(None), expected);
    assert_eq!(TheaterStrategy::from_setting(Some("nonsense")), expected);
    assert_eq!(
        TheaterStrategy::from_setting(Some("direct")),
        TheaterStrategy::Direct
    );
}

#[test]
fn the_direct_strategy_only_toggles_full_screen() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Direct, 30);
    assert_eq!(seq.enter(true), vec![Fullscreen(true)]);
    assert!(seq.fullscreen_requested());
    assert!(!seq.is_busy());
    assert_eq!(seq.advance(Some(true), Some(true)), vec![]);
    assert_eq!(seq.leave(), vec![Fullscreen(false)]);
    assert!(!seq.fullscreen_requested());
    assert!(!seq.is_busy());
}

#[test]
fn the_decorations_strategy_hides_and_restores_the_decorations() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Decorations, 30);
    assert_eq!(seq.enter(true), vec![Fullscreen(true), Decorations(false)]);
    assert_eq!(seq.leave(), vec![Fullscreen(false), Decorations(true)]);
    assert!(
        !seq.is_busy(),
        "this strategy never touches the maximized state"
    );
}

#[test]
fn the_unmaximize_strategy_acts_like_decorations_for_a_normal_window() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Unmaximize, 30);
    assert_eq!(seq.enter(false), vec![Fullscreen(true), Decorations(false)]);
    assert_eq!(seq.leave(), vec![Fullscreen(false), Decorations(true)]);
    assert!(!seq.is_busy());
    assert_eq!(seq.advance(Some(false), Some(false)), vec![]);
}

#[test]
fn a_maximized_window_is_restored_before_full_screen_and_maximized_again_afterwards() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Unmaximize, 30);
    assert_eq!(seq.enter(true), vec![Maximize(false)]);
    assert!(seq.is_busy());
    assert!(
        !seq.fullscreen_requested(),
        "the layout must not change yet"
    );
    assert_eq!(seq.advance(Some(true), Some(false)), vec![]);
    assert_eq!(seq.advance(Some(true), Some(false)), vec![]);
    assert_eq!(
        seq.advance(Some(false), Some(false)),
        vec![Fullscreen(true), Decorations(false)]
    );
    assert!(seq.fullscreen_requested());
    assert!(!seq.is_busy());

    assert_eq!(seq.leave(), vec![Fullscreen(false), Decorations(true)]);
    assert!(!seq.fullscreen_requested());
    assert!(seq.is_busy(), "waiting to maximize again");
    assert_eq!(seq.advance(Some(false), Some(true)), vec![]);
    assert_eq!(seq.advance(Some(false), Some(false)), vec![Maximize(true)]);
    assert!(!seq.is_busy());
}

#[test]
fn a_window_that_never_reports_its_state_is_handled_by_the_timeout() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Unmaximize, 3);
    assert_eq!(seq.enter(true), vec![Maximize(false)]);
    for _ in 0..3 {
        assert_eq!(seq.advance(None, None), vec![]);
    }
    assert_eq!(
        seq.advance(None, None),
        vec![Fullscreen(true), Decorations(false)]
    );
    seq.leave();
    for _ in 0..3 {
        assert_eq!(seq.advance(None, None), vec![]);
    }
    assert_eq!(seq.advance(None, None), vec![Maximize(true)]);
}

#[test]
fn cancelling_while_restoring_puts_the_window_back_and_never_enters_full_screen() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Unmaximize, 30);
    assert_eq!(seq.enter(true), vec![Maximize(false)]);
    assert_eq!(seq.leave(), vec![Maximize(true)]);
    assert!(!seq.is_busy());
    assert!(!seq.fullscreen_requested());
    assert_eq!(seq.advance(Some(false), Some(false)), vec![]);
}

#[test]
fn entering_again_while_waiting_to_maximize_goes_straight_back_to_full_screen() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Unmaximize, 30);
    seq.enter(true);
    seq.advance(Some(false), Some(false));
    seq.leave();
    assert!(seq.is_busy());
    assert_eq!(seq.enter(false), vec![Fullscreen(true), Decorations(false)]);
    assert!(!seq.is_busy());
    // the window was maximized originally, so it is maximized again at the end
    seq.leave();
    assert_eq!(seq.advance(Some(false), Some(false)), vec![Maximize(true)]);
}

#[test]
fn repeated_requests_are_ignored_and_leaving_without_entering_does_nothing() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Unmaximize, 30);
    assert_eq!(seq.leave(), vec![]);
    assert_eq!(seq.enter(true), vec![Maximize(false)]);
    assert_eq!(seq.enter(true), vec![], "already entering");
    seq.advance(Some(false), None);
    assert_eq!(seq.enter(false), vec![], "already in theater mode");
}

#[test]
fn the_maximized_state_is_only_restored_when_it_was_set_before() {
    let mut seq = WindowSequencer::new(TheaterStrategy::Unmaximize, 30);
    seq.enter(true);
    seq.advance(Some(false), None);
    seq.leave();
    seq.advance(None, Some(false));
    // second round with a window that was not maximized
    assert_eq!(seq.enter(false), vec![Fullscreen(true), Decorations(false)]);
    assert_eq!(seq.leave(), vec![Fullscreen(false), Decorations(true)]);
    assert!(
        !seq.is_busy(),
        "must not maximize a window that was not maximized"
    );
}
