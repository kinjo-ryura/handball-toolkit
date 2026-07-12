//! 移植元: `Tests/RecorderDomainTests/LiveMatchProjectionTests.swift`。

mod fixtures;

use fixtures::{
    epoch, make_video_match, phase_start_both, shootout_phase, video_only_phase, video_stoppage,
};
use handball_toolkit::clock::{FactAnchor, VideoClock};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, StoppageKind, StoppagePayload,
};
use handball_toolkit::projection::{
    LiveMatchProjection, MatchTimerState, SegmentResolver, TimelineProjection,
};
use uuid::Uuid;

// ── beforeMatch ──

#[test]
fn none_current_video_clock_returns_before_match() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &TimelineProjection {
            resolved_facts: vec![],
            resolver: SegmentResolver {
                segments: vec![],
                phases: vec![],
            },
        },
        None,
    );
    assert_eq!(live.timer_state, MatchTimerState::BeforeMatch);
    assert_eq!(live.current_phase_kind, None);
    assert!(live.available_actions.can_start_next_phase);
}

#[test]
fn current_video_before_any_phase_returns_before_match() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[phase_start_both(
            PhaseKind::Regular,
            0.0,
            720.0,
            1800.0,
            2520.0,
        )],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 100.0,
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::BeforeMatch);
}

// ── playing ──

#[test]
fn video_inside_running_phase_returns_playing() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[phase_start_both(
            PhaseKind::Regular,
            0.0,
            720.0,
            1800.0,
            2520.0,
        )],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 750.0,
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::Playing);
    assert_eq!(live.current_phase_kind, Some(PhaseKind::Regular));
    assert_eq!(live.current_phase_index, Some(0));
    assert_eq!(
        live.current_match_clock.map(|c| c.elapsed_seconds),
        Some(30.0)
    );
    assert!(live.available_actions.can_record_goal);
    assert!(live.available_actions.can_start_timeout);
}

// ── Stoppage 中の状態 ──

#[test]
fn video_inside_timeout_segment_returns_timeout_state() {
    // Phase video 0-1800、timeout video 600-660、現在 video=630
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let phase_id = Uuid::new_v4();
    let stoppage_id = Uuid::new_v4();
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[
            fixtures::video_phase(phase_id, 0.0, 1800.0),
            video_stoppage(stoppage_id, StoppageKind::Timeout, 600.0, 660.0),
        ],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 630.0,
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::Timeout);
    assert!(live.available_actions.can_resume);
    assert!(!live.available_actions.can_record_goal);
    assert_eq!(
        live.current_match_clock.map(|c| c.elapsed_seconds),
        Some(600.0)
    ); // stopped 区間は固定
}

#[test]
fn video_inside_pause_segment_returns_paused_state() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let pause_with_note = MatchFact {
        id: Uuid::new_v4(),
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind: StoppageKind::Pause,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: 800.0,
            }),
            end_anchor: Some(FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: 860.0,
            })),
            note: Some("VAR チェック".to_owned()),
        })),
    };
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[video_only_phase(0.0, 1800.0), pause_with_note],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 830.0,
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::Paused);
    assert!(live.available_actions.can_resume);
}

// ── phase 間 / 試合終了 ──

#[test]
fn video_between_phases_returns_between_phases() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[
            video_only_phase(0.0, 1800.0),
            video_only_phase(2700.0, 4500.0),
        ],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 2000.0, // ハーフタイム中
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::BetweenPhases);
    assert!(live.available_actions.can_start_next_phase);
}

#[test]
fn video_after_last_phase_returns_ended() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[video_only_phase(0.0, 1800.0)],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 2000.0, // phase 後
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::Ended);
}

// ── shootout ──

/// shootout segment 上の video は playing + shootout + phaseIndex None + 固定 matchClock を返す。
#[test]
fn video_inside_shootout_segment_returns_playing_shootout() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[
            video_only_phase(0.0, 1800.0),  // 前半 regular
            shootout_phase(2400.0, 3000.0), // shootout
        ],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 2700.0,
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::Playing); // shootout の running segment
    assert_eq!(live.current_phase_kind, Some(PhaseKind::Shootout));
    assert_eq!(live.current_phase_index, None); // shootout は regular カウント外
    assert_eq!(
        live.current_match_clock.map(|c| c.elapsed_seconds),
        Some(1800.0)
    ); // degenerate 固定
}

// ── 境界値 (firstStart / lastEnd ちょうど) ──

/// 最初の phase の videoStart ちょうどは segment 内 (playing)。
#[test]
fn video_exactly_at_first_phase_start_returns_playing() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[video_only_phase(0.0, 1800.0)],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 0.0,
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::Playing);
    assert_eq!(
        live.current_match_clock.map(|c| c.elapsed_seconds),
        Some(0.0)
    );
}

/// 最後の phase の videoEnd ちょうど (half-open の排他端) は ended。
#[test]
fn video_exactly_at_last_phase_end_returns_ended() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[video_only_phase(0.0, 1800.0)],
    );
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 1800.0,
        }),
    );
    assert_eq!(live.timer_state, MatchTimerState::Ended);
}
