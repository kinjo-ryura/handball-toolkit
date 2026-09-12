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

// ── timer / videoHighlight（handball-project#354）──

/// 3 構成の `AvailableActions` を 1 枚の表で固定する。
///
/// **この表が `RecordPolicy`（HandballRecorder の `RecorderUIShared`）の移管元**。
/// #354 以前は `.timer` / `.videoHighlight` の可否が Swift のリテラルとして UI パッケージに
/// あり、コア側で R6 / R7 / R8 / R9 の適用範囲が変わっても**コアのテストは緑のまま UI だけが
/// 古い規則で動いた**（handball-project#202 が同じ形で表面化した）。ここで表にしておくと、
/// `available_actions_for` を触ったときに 3 構成ぶんが同時に落ちる。
///
/// 凍結オラクル（Swift `RecorderDomain`）には timer / highlight の `AvailableActions` が無く
/// （`build_video_mode` しか持たない）、golden コーパス経由のパリティ検証は掛けられない。
/// **一致を見る相手は移管前の `RecordPolicyTests` の期待値**で、それをこの表へ写してある。
#[test]
fn available_actions_table_for_three_configurations() {
    // (can_record_goal, can_record_shot_missed, can_record_free_note,
    //  can_record_possession, can_start_timeout, can_resume, can_start_next_phase)
    let video_playing = {
        let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
        let timeline = TimelineProjection::build(
            &make_video_match(home, away),
            &[video_only_phase(0.0, 1800.0)],
        );
        LiveMatchProjection::build_video_mode(
            &make_video_match(home, away),
            &timeline,
            Some(VideoClock {
                elapsed_seconds: 30.0,
            }),
        )
        .available_actions
    };

    let timer = LiveMatchProjection::build_timer_mode().available_actions;
    let highlight = LiveMatchProjection::build_highlight_mode().available_actions;

    // 記録系（単一 anchor fact）は 3 構成とも同じ。`.timer` は matchClock 座標で停止区間が
    // 幅ゼロになり R8 が、phase が D-snap で auto-create されるので R7 が構造上当たらない。
    // `.videoHighlight` は R7 / R8 がそもそも適用対象外（handball-project#138）。
    for (label, actions) in [("timer", timer), ("highlight", highlight)] {
        assert!(actions.can_record_goal, "{label}");
        assert!(actions.can_record_shot_missed, "{label}");
        assert!(actions.can_record_free_note, "{label}");
        // 記録の導線を出すかは各シェルの判断。コアは「保存が通るか」を返す
        // （`.videoHighlight` で導線を出さない判断は handball-project#202 のシェル側）。
        assert!(actions.can_record_possession, "{label}");
    }
    assert!(video_playing.can_record_goal);
    assert!(video_playing.can_record_possession);

    // 停止区間: `.timer` は可（endAnchor nil で進行中を表す）、`.videoHighlight` は R9 で禁止。
    assert!(timer.can_start_timeout);
    assert!(!highlight.can_start_timeout);
    assert!(video_playing.can_start_timeout);

    // phase 開始: `.timer` は直前 phase の end と連続する PhaseStart が valid なので可。
    // `.videoHighlight` は R6 で禁止。`.video` は phase の内側なので不可。
    assert!(timer.can_start_next_phase);
    assert!(!highlight.can_start_next_phase);
    assert!(!video_playing.can_start_next_phase);

    // 再開は video mode の停止区間でのみ立つフラグ。
    assert!(!timer.can_resume);
    assert!(!highlight.can_resume);
    assert!(!video_playing.can_resume);
}

/// `Playing` 行から派生していること。`available_actions_for(Playing)` の記録系を変えたら
/// timer / highlight も一緒に動く（取り残されない）ことを固定する。
///
/// handball-project#177 は `Playing` 行だけを直して Swift のリテラルが取り残された事例で、
/// #354 はその取り残しを構造的に起こせなくするのが目的。
#[test]
fn timer_and_highlight_derive_recording_flags_from_playing() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let timeline = TimelineProjection::build(
        &make_video_match(home, away),
        &[video_only_phase(0.0, 1800.0)],
    );
    let playing = LiveMatchProjection::build_video_mode(
        &make_video_match(home, away),
        &timeline,
        Some(VideoClock {
            elapsed_seconds: 30.0,
        }),
    )
    .available_actions;

    for actions in [
        LiveMatchProjection::build_timer_mode().available_actions,
        LiveMatchProjection::build_highlight_mode().available_actions,
    ] {
        assert_eq!(actions.can_record_goal, playing.can_record_goal);
        assert_eq!(
            actions.can_record_shot_missed,
            playing.can_record_shot_missed
        );
        assert_eq!(actions.can_record_free_note, playing.can_record_free_note);
        assert_eq!(actions.can_record_possession, playing.can_record_possession);
    }
}

/// 位置を入力に取らないので、呼ぶたびに同じ値を返す。
///
/// **`current_match_clock` は返さない**（試合時計の現在値はシェルが自分のタイマーで持っており
/// fact 列からは導けない）。ここを `Some` にすると、消費側の時刻表示が自前のタイマーから
/// コアの値へ黙って切り替わる。
#[test]
fn timer_and_highlight_projections_are_position_independent() {
    assert_eq!(
        LiveMatchProjection::build_timer_mode(),
        LiveMatchProjection::build_timer_mode()
    );
    assert_eq!(
        LiveMatchProjection::build_highlight_mode(),
        LiveMatchProjection::build_highlight_mode()
    );

    let timer = LiveMatchProjection::build_timer_mode();
    assert_eq!(timer.timer_state, MatchTimerState::Playing);
    assert_eq!(timer.current_match_clock, None);
    assert_eq!(timer.current_phase_kind, None);
    assert_eq!(timer.current_phase_index, None);

    // ハイライト集には時計の状態が無い。`MatchTimerState` に「時計なし」の case は無いので、
    // phase が 1 つも無い log に `build_video_mode` を当てたときと同じ `BeforeMatch` を返す
    // （移管で消費側の表示を変えないため）。
    let highlight = LiveMatchProjection::build_highlight_mode();
    assert_eq!(highlight.timer_state, MatchTimerState::BeforeMatch);
    assert_eq!(highlight.current_match_clock, None);
    assert_eq!(highlight.current_phase_kind, None);
    assert_eq!(highlight.current_phase_index, None);

    let empty = LiveMatchProjection::build_video_mode_with_resolver(
        &std::sync::Arc::new(SegmentResolver {
            segments: vec![],
            phases: vec![],
        }),
        Some(VideoClock {
            elapsed_seconds: 42.0,
        }),
    );
    assert_eq!(highlight.timer_state, empty.timer_state);
    assert_eq!(highlight.current_match_clock, empty.current_match_clock);
}
