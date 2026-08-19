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
use handball_toolkit::ids::{FactId, TeamId};
use handball_toolkit::projection::{
    LiveMatchProjection, MatchTimerState, SegmentResolver, TimelineProjection,
};
use uuid::Uuid;

// ── beforeMatch ──

#[test]
fn none_current_video_clock_returns_before_match() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let live = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &TimelineProjection {
            resolved_facts: vec![],
            resolver: std::sync::Arc::new(SegmentResolver {
                segments: vec![],
                phases: vec![],
            }),
        },
        None,
    );
    assert_eq!(live.timer_state, MatchTimerState::BeforeMatch);
    assert_eq!(live.current_phase_kind, None);
    assert!(live.available_actions.can_start_next_phase);
}

#[test]
fn current_video_before_any_phase_returns_before_match() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
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
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
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
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let phase_id = FactId(Uuid::new_v4());
    let stoppage_id = FactId(Uuid::new_v4());
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
    assert!(!live.available_actions.can_record_free_note);
    assert_eq!(
        live.current_match_clock.map(|c| c.elapsed_seconds),
        Some(600.0)
    ); // stopped 区間は固定
}

#[test]
fn video_inside_pause_segment_returns_paused_state() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let pause_with_note = MatchFact {
        id: FactId(Uuid::new_v4()),
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
    assert!(!live.available_actions.can_record_free_note);
}

// ── phase 間 / 試合終了 ──

#[test]
fn video_between_phases_returns_between_phases() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
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
    assert!(!live.available_actions.can_record_free_note);
}

#[test]
fn video_after_last_phase_returns_ended() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
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
    assert!(!live.available_actions.can_record_free_note);
}

// ── play fact 3 種の可否は常に同値 ──

/// goal / shotMissed / freeNote の可否は全状態で同値で、`Playing` でのみ true。
///
/// 移植元 Swift は freeNote だけ停止区間 / phase 間 / 試合終了後でも true にしていたが、
/// R7 / R8 は kind を問わず掛かるためコアの validation と矛盾していた（handball-project#177）。
/// フラグを素直に読む消費者（Android シェル等）が「記録できる」と案内して保存で落ちる経路を、
/// ここで再発しないよう固定する。
#[test]
fn play_fact_flags_agree_and_are_true_only_while_playing() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let stoppage_id = FactId(Uuid::new_v4());
    // 前半 video 0-1800（timeout 600-660）、後半 2700-4500
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[
            video_only_phase(0.0, 1800.0),
            video_stoppage(stoppage_id, StoppageKind::Timeout, 600.0, 660.0),
            video_only_phase(2700.0, 4500.0),
        ],
    );
    let cases = [
        (None, MatchTimerState::BeforeMatch),
        (Some(30.0), MatchTimerState::Playing),
        (Some(630.0), MatchTimerState::Timeout),
        (Some(2000.0), MatchTimerState::BetweenPhases),
        (Some(5000.0), MatchTimerState::Ended),
    ];
    for (video_seconds, expected_state) in cases {
        let live = LiveMatchProjection::build_video_mode(
            &make_video_match(home, away),
            &timeline,
            video_seconds.map(|elapsed_seconds| VideoClock { elapsed_seconds }),
        );
        assert_eq!(live.timer_state, expected_state);
        let actions = live.available_actions;
        let expected = expected_state == MatchTimerState::Playing;
        assert_eq!(actions.can_record_goal, expected, "{expected_state:?}");
        assert_eq!(
            actions.can_record_shot_missed, expected,
            "{expected_state:?}"
        );
        assert_eq!(actions.can_record_free_note, expected, "{expected_state:?}");
        // ポゼッション開始も単一 anchor fact なので R7 / R8 が同じく掛かる（handball-project#184）。
        assert_eq!(
            actions.can_record_possession, expected,
            "{expected_state:?}"
        );
    }
}

// ── shootout ──

/// shootout segment 上の video は playing + shootout + phaseIndex None + 固定 matchClock を返す。
#[test]
fn video_inside_shootout_segment_returns_playing_shootout() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
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
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
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
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
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

// ── 2 経路の一致（handball-project#167）──

/// **resolver だけを受ける入口と、timeline を受ける入口の結果が一致すること。**
///
/// FFI の 2Hz tick 経路は `build_video_mode_with_resolver` を呼ぶ（`TimelineProjection` を
/// record ごと渡すと resolver ハンドルの手前で `resolved_facts` が全量マーシャリングされ、
/// fact 列が毎 tick 境界を渡るため）。一方 golden parity とオラクル対応のテストは
/// `build_video_mode` を呼び続ける。**2 経路が育って食い違わないこと**をここで固定する。
///
/// 位置は状態が分かれる点を一通り踏む — phase 前 / phase 内 / 停止区間の内側 /
/// phase 間 / 試合終了後、および `None`。
#[test]
fn resolver_entry_point_matches_timeline_entry_point() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let match_ = make_video_match(home, away);
    let timeline = TimelineProjection::build(
        &match_,
        &[
            phase_start_both(PhaseKind::Regular, 0.0, 720.0, 1800.0, 2520.0),
            video_stoppage(
                FactId(Uuid::new_v4()),
                StoppageKind::Timeout,
                1000.0,
                1060.0,
            ),
            phase_start_both(PhaseKind::Regular, 1800.0, 3000.0, 3600.0, 4800.0),
        ],
    );

    let positions = [
        None,
        Some(100.0),  // phase 前
        Some(800.0),  // 1st phase 内
        Some(1030.0), // タイムアウト区間の内側
        Some(2700.0), // phase 間（ハーフタイム）
        Some(3500.0), // 2nd phase 内
        Some(4800.0), // 最後の phase の end ちょうど = 終了後
        Some(9000.0), // 試合終了後
    ];

    for position in positions {
        let clock = position.map(|elapsed_seconds| VideoClock { elapsed_seconds });
        let via_timeline = LiveMatchProjection::build_video_mode(&match_, &timeline, clock);
        let via_resolver =
            LiveMatchProjection::build_video_mode_with_resolver(&timeline.resolver, clock);
        assert_eq!(
            via_timeline, via_resolver,
            "位置 {position:?} で 2 経路の結果が食い違った"
        );
    }
}
