//! 記録入口の純粋ヘルパー（`write` の後段 — handball-project#69）の挙動固定。
//!
//! 移植元は `RecordingScreenStore` に残っていた小さなドメイン計算と、それを固定していた
//! `RecordingScreenStoreEditTests` / `RecordingScreenStoreTests`。挙動は「改善」せず
//! Swift 実装と一致させる（PORTING.md 作業規律 / ADR 0005 決定 7）。
//!
//! `tickTimer` の delta 加算はシェル（UI 状態遷移・2Hz 経路）に残したため、
//! `RecordingScreenStoreTickTests` は Swift 側に据え置く。

use chrono::{DateTime, Utc};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
use handball_toolkit::ids::{FactId, PlayerId, TeamId};
use handball_toolkit::write::{
    CaptureClockKind, NewFactStamp, PlayFactEdit, apply_play_fact_edit, build_play_fact,
    build_stoppage_fact, capture_play_anchor, initial_timer_seconds,
};
use uuid::Uuid;

fn epoch() -> DateTime<Utc> {
    DateTime::from_timestamp(0, 0).unwrap()
}

fn stamp(n: u128) -> NewFactStamp {
    NewFactStamp {
        id: FactId(Uuid::from_u128(n)),
        recorded_at: epoch(),
    }
}

fn play(anchor: FactAnchor) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(900)),
        recorded_at: epoch(),
        payload: MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: Some(TeamId(Uuid::from_u128(1))),
            player_id: Some(PlayerId(Uuid::from_u128(11))),
            related_player_id: None,
            anchor,
            title: None,
            note: None,
        }),
    }
}

fn goal_play(anchor: FactAnchor) -> PlayFact {
    PlayFact {
        kind: PlayEventKind::Goal,
        team_id: Some(TeamId(Uuid::from_u128(1))),
        player_id: Some(PlayerId(Uuid::from_u128(11))),
        related_player_id: None,
        anchor,
        title: None,
        note: None,
    }
}

fn phase_start(start: f64, end: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(800)),
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: start,
            }),
            end_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: end,
            }),
        })),
    }
}

// ── capture_play_anchor（移植元: capturePlayEvent / recordFreeNote / capturePlayEventInVideoMode）──

#[test]
fn 記録_offset_を基準秒から引いて_match_clock_anchor_を組む() {
    let anchor = capture_play_anchor(600.0, 5.0, CaptureClockKind::MatchClock, &[]);
    assert_eq!(
        anchor,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 595.0
        })
    );
}

#[test]
fn 記録_offset_を基準秒から引いて_video_clock_anchor_を組む() {
    let anchor = capture_play_anchor(300.0, 5.0, CaptureClockKind::VideoClock, &[]);
    assert_eq!(
        anchor,
        FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 295.0
        })
    );
}

#[test]
fn offset_が基準秒を上回っても_0_にクランプする() {
    let anchor = capture_play_anchor(3.0, 5.0, CaptureClockKind::MatchClock, &[]);
    assert_eq!(
        anchor,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 0.0
        })
    );
}

#[test]
fn offset_0_なら基準秒がそのまま_anchor_になる() {
    let anchor = capture_play_anchor(42.5, 0.0, CaptureClockKind::VideoClock, &[]);
    assert_eq!(
        anchor,
        FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 42.5
        })
    );
}

// ── capture_play_anchor の境界クランプ（handball-project#101）──
//
// オフセットは phase 境界 / stoppage 区間を越えない。越えると動画モードでは R7 / R8 で
// 保存が拒否されて記録が失われ、タイマーモードでは前 phase の得点として静かに集計される。

fn video_phase_start(id: u128, start: f64, end: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: start,
            }),
            end_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end,
            }),
        })),
    }
}

fn video_stoppage(id: u128, start: f64, end: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind: StoppageKind::Timeout,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: start,
            }),
            end_anchor: Some(FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end,
            })),
            note: None,
        })),
    }
}

fn video_seconds(anchor: FactAnchor) -> f64 {
    match anchor {
        FactAnchor::VideoClock(clock) => clock.elapsed_seconds,
        other => panic!("videoClock anchor を期待したが {other:?} だった"),
    }
}

fn match_seconds(anchor: FactAnchor) -> f64 {
    match anchor {
        FactAnchor::MatchClock(clock) => clock.elapsed_seconds,
        other => panic!("matchClock anchor を期待したが {other:?} だった"),
    }
}

#[test]
fn 動画_phase_開始直後の記録は_phase_開始で止まる() {
    let facts = vec![video_phase_start(801, 600.0, 2400.0)];
    // 開始 1 秒後にタップ。素の減算なら 598.0 で phase 範囲外 → R7 で拒否されていた。
    let anchor = capture_play_anchor(601.0, 3.0, CaptureClockKind::VideoClock, &facts);
    assert_eq!(video_seconds(anchor), 600.0);
}

#[test]
fn 動画_phase_内に余裕があればクランプしない() {
    let facts = vec![video_phase_start(801, 600.0, 2400.0)];
    let anchor = capture_play_anchor(620.0, 3.0, CaptureClockKind::VideoClock, &facts);
    assert_eq!(video_seconds(anchor), 617.0);
}

#[test]
fn 動画_停止明け直後の記録は_停止終了で止まる() {
    let facts = vec![
        video_phase_start(801, 0.0, 1800.0),
        video_stoppage(802, 500.0, 600.0),
    ];
    // 停止明け 1 秒後にタップ。素の減算なら 598.0 で停止区間の内側 → R8 で拒否されていた。
    let anchor = capture_play_anchor(601.0, 3.0, CaptureClockKind::VideoClock, &facts);
    assert_eq!(video_seconds(anchor), 600.0);
}

#[test]
fn 動画_停止区間より十分後ならクランプしない() {
    let facts = vec![
        video_phase_start(801, 0.0, 1800.0),
        video_stoppage(802, 500.0, 600.0),
    ];
    let anchor = capture_play_anchor(700.0, 3.0, CaptureClockKind::VideoClock, &facts);
    assert_eq!(video_seconds(anchor), 697.0);
}

#[test]
fn タイマー_後半開始直後の記録が前半に食い込まない() {
    // phase は matchClock 上で連続する（前 phase の end == 次 phase の start）。
    let facts = vec![phase_start(1800.0, 3600.0)];
    let anchor = capture_play_anchor(1801.0, 3.0, CaptureClockKind::MatchClock, &facts);
    assert_eq!(match_seconds(anchor), 1800.0);
}

#[test]
fn 押した位置がどの_phase_にも属さないならクランプしない() {
    // ハーフタイム中の記録などは validation に委ねる。ここで境界へ寄せると
    // 「範囲外に記録した」事実が消えてしまう。
    let facts = vec![video_phase_start(801, 0.0, 1800.0)];
    let anchor = capture_play_anchor(1900.0, 3.0, CaptureClockKind::VideoClock, &facts);
    assert_eq!(video_seconds(anchor), 1897.0);
}

#[test]
fn 時計種別が異なる_fact_は境界に使わない() {
    // タイマーモードの記録に対して videoClock だけの phase fact は無関係。
    let facts = vec![video_phase_start(801, 600.0, 2400.0)];
    let anchor = capture_play_anchor(601.0, 3.0, CaptureClockKind::MatchClock, &facts);
    assert_eq!(match_seconds(anchor), 598.0);
}

// ── initial_timer_seconds（移植元: lastPlayMatchClock + load() の ?? 0）──

#[test]
fn play_fact_が無ければ初期累積秒は_0() {
    assert_eq!(initial_timer_seconds(&[]), 0.0);
    assert_eq!(initial_timer_seconds(&[phase_start(0.0, 1800.0)]), 0.0);
}

#[test]
fn 直近_play_の_match_clock_を初期累積秒にする() {
    let facts = [
        phase_start(0.0, 1800.0),
        play(FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 300.0,
        })),
        play(FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 600.0,
        })),
    ];
    assert_eq!(initial_timer_seconds(&facts), 600.0);
}

#[test]
fn 直近_play_より後ろの_control_fact_は読み飛ばす() {
    let facts = [
        play(FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 600.0,
        })),
        phase_start(1800.0, 3600.0),
    ];
    assert_eq!(initial_timer_seconds(&facts), 600.0);
}

/// 移植元は「最初に見つかった play」で早期 return し、その anchor に matchClock が無ければ
/// nil を返す（さらに前の play を探しに行かない）。この非直感的な挙動もそのまま写す。
#[test]
fn 直近_play_が_video_clock_単独なら手前に_match_clock_があっても_0() {
    let facts = [
        play(FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 600.0,
        })),
        play(FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 900.0,
        })),
    ];
    assert_eq!(initial_timer_seconds(&facts), 0.0);
}

#[test]
fn 直近_play_が_both_anchor_なら_match_clock_側を使う() {
    let facts = [play(FactAnchor::Both {
        match_clock: MatchClock {
            elapsed_seconds: 600.0,
        },
        video_clock: VideoClock {
            elapsed_seconds: 1200.0,
        },
    })];
    assert_eq!(initial_timer_seconds(&facts), 600.0);
}

// ── build_play_fact / build_stoppage_fact ──

#[test]
fn play_fact_は_stamp_の_id_と_recorded_at_を載せる() {
    let fact = build_play_fact(
        stamp(1),
        PlayEventKind::Goal,
        Some(TeamId(Uuid::from_u128(1))),
        Some(PlayerId(Uuid::from_u128(11))),
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 600.0,
        }),
        None,
        None,
    );

    assert_eq!(fact.id, FactId(Uuid::from_u128(1)));
    assert_eq!(fact.recorded_at, epoch());
    let MatchFactPayload::Play(play) = fact.payload else {
        panic!("play fact のはず");
    };
    assert_eq!(play.kind, PlayEventKind::Goal);
    assert_eq!(play.team_id, Some(TeamId(Uuid::from_u128(1))));
    assert_eq!(play.player_id, Some(PlayerId(Uuid::from_u128(11))));
    assert_eq!(play.related_player_id, None);
}

/// 移植元は新規記録経路だけ正規化しておらず、同じ文字列でも新規記録か編集かで
/// 保存される中身が変わる非対称があった（handball-project#69 で解消）。
#[test]
fn 新規_play_fact_の_title_と_note_も編集と同じ規則で正規化する() {
    let fact = build_play_fact(
        stamp(2),
        PlayEventKind::FreeNote,
        None,
        None,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 0.0,
        }),
        Some("  タイトル  ".to_string()),
        Some("   ".to_string()),
    );

    let MatchFactPayload::Play(play) = fact.payload else {
        panic!("play fact のはず");
    };
    assert_eq!(play.title.as_deref(), Some("タイトル"));
    assert_eq!(play.note, None);
}

#[test]
fn タイマーモードの_stoppage_は_end_anchor_なしで組む() {
    let fact = build_stoppage_fact(
        stamp(3),
        StoppageKind::Timeout,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 600.0,
        }),
        None,
        None,
    );

    let MatchFactPayload::Control(ControlFact::Stoppage(payload)) = fact.payload else {
        panic!("stoppage fact のはず");
    };
    assert_eq!(payload.kind, StoppageKind::Timeout);
    assert_eq!(payload.end_anchor, None);
    assert_eq!(payload.note, None);
}

#[test]
fn 動画モードの_stoppage_は_end_anchor_付きで組む() {
    let fact = build_stoppage_fact(
        stamp(4),
        StoppageKind::Pause,
        FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 300.0,
        }),
        Some(FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 360.0,
        })),
        Some("負傷".to_string()),
    );

    let MatchFactPayload::Control(ControlFact::Stoppage(payload)) = fact.payload else {
        panic!("stoppage fact のはず");
    };
    assert_eq!(
        payload.end_anchor,
        Some(FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 360.0
        }))
    );
    assert_eq!(payload.note.as_deref(), Some("負傷"));
}

/// stoppage の note も play fact と同じ規則で正規化する（移植元は
/// `recordTimerPause` だけ trim し `recordVideoStoppage` は素通しだった）。
#[test]
fn 新規_stoppage_fact_の_note_も正規化する() {
    let trimmed = build_stoppage_fact(
        stamp(5),
        StoppageKind::Pause,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 600.0,
        }),
        None,
        Some("  怪我  ".to_string()),
    );
    let MatchFactPayload::Control(ControlFact::Stoppage(payload)) = trimmed.payload else {
        panic!("stoppage fact のはず");
    };
    assert_eq!(payload.note.as_deref(), Some("怪我"));

    let blank = build_stoppage_fact(
        stamp(6),
        StoppageKind::Pause,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 600.0,
        }),
        None,
        Some("   ".to_string()),
    );
    let MatchFactPayload::Control(ControlFact::Stoppage(payload)) = blank.payload else {
        panic!("stoppage fact のはず");
    };
    assert_eq!(payload.note, None);
}

// ── apply_play_fact_edit（移植元: RecordingScreenStoreEditTests）──

/// 移植元: `updateFactNoteTrimsAndStores`。
#[test]
fn note_編集は前後の空白を除去して格納する() {
    let base = goal_play(FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: 600.0,
    }));
    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::Note {
            text: Some("  メモ  ".to_string()),
        },
    );
    assert_eq!(edited.note.as_deref(), Some("メモ"));
}

/// 移植元: `updateFactNoteToBlankClearsNote`。
#[test]
fn note_編集で空白のみなら_none_になる() {
    let mut base = goal_play(FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: 600.0,
    }));
    base.note = Some("メモ".to_string());

    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::Note {
            text: Some("   ".to_string()),
        },
    );
    assert_eq!(edited.note, None);
}

#[test]
fn title_編集も_trim_と空文字_none_化をする() {
    let base = goal_play(FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: 600.0,
    }));
    let trimmed = apply_play_fact_edit(
        base.clone(),
        PlayFactEdit::Title {
            text: Some("  タイトル\n".to_string()),
        },
    );
    assert_eq!(trimmed.title.as_deref(), Some("タイトル"));

    let cleared = apply_play_fact_edit(
        base,
        PlayFactEdit::Title {
            text: Some(String::new()),
        },
    );
    assert_eq!(cleared.title, None);
}

/// 移植元: `updateFactPlayerChangesPlayerStat`（コア側は選手 ID の差し替えのみを固定）。
#[test]
fn player_編集は選手_id_を差し替える() {
    let base = goal_play(FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: 600.0,
    }));
    let bob = PlayerId(Uuid::from_u128(22));

    let edited = apply_play_fact_edit(
        base.clone(),
        PlayFactEdit::Player {
            player_id: Some(bob),
        },
    );
    assert_eq!(edited.player_id, Some(bob));

    let cleared = apply_play_fact_edit(base, PlayFactEdit::Player { player_id: None });
    assert_eq!(cleared.player_id, None);
}

/// 移植元: `updateFactKindFromGoalToShotMissedDecrementsScore`（コア側は kind 差し替えのみ。
/// score への波及は summary projection のテストが担保する）。
#[test]
fn kind_編集はイベント種別を差し替える() {
    let base = goal_play(FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: 600.0,
    }));
    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::Kind {
            kind: PlayEventKind::ShotMissed,
        },
    );
    assert_eq!(edited.kind, PlayEventKind::ShotMissed);
}

/// 移植元: `updateFactMatchClockChangesAnchor`。
#[test]
fn match_clock_編集は_anchor_を_match_clock_単独にする() {
    let base = goal_play(FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: 600.0,
    }));
    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::MatchClock {
            elapsed_seconds: 1500.0,
        },
    );
    assert_eq!(
        edited.anchor,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 1500.0
        })
    );
}

#[test]
fn match_clock_編集の負値は_0_にクランプする() {
    let base = goal_play(FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: 600.0,
    }));
    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::MatchClock {
            elapsed_seconds: -10.0,
        },
    );
    assert_eq!(
        edited.anchor,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 0.0
        })
    );
}

/// 移植元: `updateFactVideoClockReplacesVideoClockAnchor`。
#[test]
fn video_clock_編集は_video_clock_単独_anchor_を差し替える() {
    let base = goal_play(FactAnchor::VideoClock(VideoClock {
        elapsed_seconds: 300.0,
    }));
    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::VideoClock {
            elapsed_seconds: 720.0,
        },
    );
    assert_eq!(
        edited.anchor,
        FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 720.0
        })
    );
}

/// 移植元: `updateFactVideoClockOnBothAnchorPreservesMatchClock`。
#[test]
fn video_clock_編集は_both_anchor_の_match_clock_を保持する() {
    let base = goal_play(FactAnchor::Both {
        match_clock: MatchClock {
            elapsed_seconds: 600.0,
        },
        video_clock: VideoClock {
            elapsed_seconds: 1200.0,
        },
    });
    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::VideoClock {
            elapsed_seconds: 1800.0,
        },
    );
    assert_eq!(
        edited.anchor,
        FactAnchor::Both {
            match_clock: MatchClock {
                elapsed_seconds: 600.0
            },
            video_clock: VideoClock {
                elapsed_seconds: 1800.0
            },
        }
    );
}

/// 移植元: `updateFactVideoClockOnMatchClockAnchorIsNoOp`。
#[test]
fn video_clock_編集は_match_clock_単独_anchor_を変更しない() {
    let base = goal_play(FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: 600.0,
    }));
    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::VideoClock {
            elapsed_seconds: 999.0,
        },
    );
    assert_eq!(
        edited.anchor,
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 600.0
        })
    );
}

#[test]
fn video_clock_編集の負値も_0_にクランプする() {
    let base = goal_play(FactAnchor::VideoClock(VideoClock {
        elapsed_seconds: 300.0,
    }));
    let edited = apply_play_fact_edit(
        base,
        PlayFactEdit::VideoClock {
            elapsed_seconds: -1.0,
        },
    );
    assert_eq!(
        edited.anchor,
        FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 0.0
        })
    );
}
