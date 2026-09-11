//! 計画層 `write`（純粋関数）の挙動固定（ADR 0005 実装順序 1・3）。
//!
//! - roster 構築の後方互換ルールを移植元 `SwiftDataMatchRepository.loadRosterContext` と
//!   同セマンティクスで固定する: 選手 0 件は None（参照整合 skip）・同一選手の重複は先勝ち
//! - phase 自動補完計画を移植元 `RecordingScreenStore.ensureTimerPhasesCovering`
//!   （とその store テスト群）と同セマンティクスで固定する

use std::collections::{BTreeMap, BTreeSet};

use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::{
    MatchConfiguration, MatchConfigurationKind, PhaseKind, VideoProvider, VideoSource,
};
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
use handball_toolkit::ids::{FactId, MatchId, PlayerId, TeamId};
use handball_toolkit::write::{
    NewFactStamp, PhaseDurationChangeError, PlayerTeamRef, VideoMigrationPlanError,
    VideoSourceReplacementError, VideoSyncInput, phase_completion_fact, phase_completion_plan,
    phase_duration_change_plan, roster_context_from_players, video_migration_plan,
    video_source_replacement_plan,
};
use uuid::Uuid;

#[test]
fn 選手_0_件なら_none_で参照整合を_skip_する() {
    let home = TeamId(Uuid::from_u128(1));
    let away = TeamId(Uuid::from_u128(2));
    assert_eq!(roster_context_from_players(home, away, &[]), None);
}

#[test]
fn 選手一覧から_lookup_と_known_ids_を組む() {
    let home = TeamId(Uuid::from_u128(1));
    let away = TeamId(Uuid::from_u128(2));
    let p1 = PlayerId(Uuid::from_u128(11));
    let p2 = PlayerId(Uuid::from_u128(12));
    let players = [
        PlayerTeamRef {
            player_id: p1,
            team_id: home,
        },
        PlayerTeamRef {
            player_id: p2,
            team_id: away,
        },
    ];

    let roster = roster_context_from_players(home, away, &players).expect("1 件以上なら Some");
    assert_eq!(roster.home_team_id, home);
    assert_eq!(roster.away_team_id, away);
    assert_eq!(
        roster.player_team_lookup,
        BTreeMap::from([(p1, home), (p2, away)])
    );
    assert_eq!(roster.known_player_ids, Some(BTreeSet::from([p1, p2])));
}

#[test]
fn 同一選手の重複は先勝ちで_lookup_を組む() {
    let home = TeamId(Uuid::from_u128(1));
    let away = TeamId(Uuid::from_u128(2));
    let p1 = PlayerId(Uuid::from_u128(11));
    let players = [
        PlayerTeamRef {
            player_id: p1,
            team_id: home,
        },
        PlayerTeamRef {
            player_id: p1,
            team_id: away,
        },
    ];

    let roster = roster_context_from_players(home, away, &players).expect("1 件以上なら Some");
    assert_eq!(roster.player_team_lookup, BTreeMap::from([(p1, home)]));
    assert_eq!(roster.known_player_ids, Some(BTreeSet::from([p1])));
}

// ── phase 自動補完計画（移植元: ensureTimerPhasesCovering の store テスト群）──

fn timer_match(duration: f64) -> Match {
    Match {
        id: MatchId(Uuid::from_u128(1)),
        title: None,
        date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
        home_team_id: TeamId(Uuid::from_u128(2)),
        away_team_id: TeamId(Uuid::from_u128(3)),
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: duration,
        },
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    }
}

fn goal_at(seconds: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(100)),
        recorded_at: chrono::DateTime::from_timestamp(10, 0).expect("固定秒は有効"),
        payload: MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: None,
            player_id: Some(PlayerId(Uuid::from_u128(50))),
            related_player_id: None,
            anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: seconds,
            }),
            title: None,
            note: None,
        }),
    }
}

fn phase_start(start: f64, end: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128((start as u128) + 200)),
        recorded_at: chrono::DateTime::from_timestamp(1, 0).expect("固定秒は有効"),
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

fn slots(plan: &[handball_toolkit::write::PhaseCompletionSlot]) -> Vec<(f64, f64)> {
    plan.iter()
        .map(|s| (s.start_seconds, s.end_seconds))
        .collect()
}

#[test]
fn 前半の記録は_d_snap_phase_1_を補完する() {
    let plan = phase_completion_plan(&timer_match(1800.0), &[], &goal_at(100.0));
    assert_eq!(slots(&plan), vec![(0.0, 1800.0)]);
}

#[test]
fn 後半の記録は欠けた前半も連鎖補完する() {
    let plan = phase_completion_plan(&timer_match(1800.0), &[], &goal_at(1900.0));
    assert_eq!(slots(&plan), vec![(0.0, 1800.0), (1800.0, 3600.0)]);
}

#[test]
fn 既存_phase_が満たす区間は補完しない() {
    let existing = vec![phase_start(0.0, 1800.0)];
    let plan = phase_completion_plan(&timer_match(1800.0), &existing, &goal_at(1900.0));
    assert_eq!(slots(&plan), vec![(1800.0, 3600.0)]);
}

#[test]
fn 全区間が満たされていれば空() {
    let existing = vec![phase_start(0.0, 1800.0), phase_start(1800.0, 3600.0)];
    let plan = phase_completion_plan(&timer_match(1800.0), &existing, &goal_at(1900.0));
    assert!(plan.is_empty());
}

#[test]
fn 動画モードは補完しない() {
    let mut match_ = timer_match(1800.0);
    match_.configuration = MatchConfiguration::Video(VideoSource {
        provider: VideoProvider::Youtube,
        external_id: "poc".to_string(),
    });
    let plan = phase_completion_plan(&match_, &[], &goal_at(100.0));
    assert!(plan.is_empty());
}

#[test]
fn phase_start_自身の記録は補完しない() {
    let explicit = phase_start(0.0, 1800.0);
    let plan = phase_completion_plan(&timer_match(1800.0), &[], &explicit);
    assert!(plan.is_empty());
}

#[test]
fn stoppage_の記録も補完対象() {
    let pause = MatchFact {
        id: FactId(Uuid::from_u128(101)),
        recorded_at: chrono::DateTime::from_timestamp(10, 0).expect("固定秒は有効"),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind: StoppageKind::Pause,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: 100.0,
            }),
            end_anchor: None,
            note: None,
        })),
    };
    let plan = phase_completion_plan(&timer_match(1800.0), &[], &pause);
    assert_eq!(slots(&plan), vec![(0.0, 1800.0)]);
}

#[test]
fn matchclock_anchor_が無い記録は_phase_1_のみ確保する() {
    let mut goal = goal_at(0.0);
    if let MatchFactPayload::Play(play) = &mut goal.payload {
        play.anchor = FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 100.0,
        });
    }
    // timer 構成で videoClock anchor は validation で弾かれる経路だが、
    // 計画は移植元の `?? 0` と同じく phase 1 のみを返す（判断は validator の守備範囲）。
    let plan = phase_completion_plan(&timer_match(1800.0), &[], &goal);
    assert_eq!(slots(&plan), vec![(0.0, 1800.0)]);
}

#[test]
fn 補完_fact_はスタンプの_id_と時刻で組まれる() {
    let plan = phase_completion_plan(&timer_match(1800.0), &[], &goal_at(100.0));
    let stamp = NewFactStamp {
        id: FactId(Uuid::from_u128(77)),
        recorded_at: chrono::DateTime::from_timestamp(42, 0).expect("固定秒は有効"),
    };
    let fact = phase_completion_fact(plan[0], stamp);
    assert_eq!(fact.id, stamp.id);
    assert_eq!(fact.recorded_at, stamp.recorded_at);
    match fact.payload {
        MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => {
            assert_eq!(payload.kind, PhaseKind::Regular);
            assert_eq!(
                payload.start_anchor,
                FactAnchor::MatchClock(MatchClock {
                    elapsed_seconds: 0.0
                })
            );
            assert_eq!(
                payload.end_anchor,
                FactAnchor::MatchClock(MatchClock {
                    elapsed_seconds: 1800.0
                })
            );
        }
        other => panic!("PhaseStart を期待したが {other:?}"),
    }
}

// ── video 移行 commit 計画（移植元: MigrateToVideoStore.buildUpdatedFacts）──

fn sync(fact: &MatchFact, start: f64, end: f64) -> VideoSyncInput {
    VideoSyncInput {
        fact_id: fact.id,
        video_start_seconds: start,
        video_end_seconds: end,
    }
}

#[test]
fn 移行計画は_phase_start_を_both_anchor_化し_play_を_video_clock_へ変換する() {
    let phase = phase_start(0.0, 1800.0);
    let goal = goal_at(60.0);
    let plan = video_migration_plan(
        &[phase.clone(), goal.clone()],
        &[sync(&phase, 10.0, 1810.0)],
        &[],
    )
    .expect("計画成立");

    assert_eq!(plan.len(), 2, "control → play の順");
    match &plan[0].payload {
        MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => {
            assert_eq!(
                payload.start_anchor,
                FactAnchor::Both {
                    match_clock: MatchClock {
                        elapsed_seconds: 0.0
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: 10.0
                    },
                }
            );
            assert_eq!(
                payload.end_anchor,
                FactAnchor::Both {
                    match_clock: MatchClock {
                        elapsed_seconds: 1800.0
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: 1810.0
                    },
                }
            );
        }
        other => panic!("PhaseStart を期待したが {other:?}"),
    }
    match &plan[1].payload {
        MatchFactPayload::Play(play) => {
            // baseline rolling forward: video 10 + (mc 60 - mc 0) = 70。
            assert_eq!(
                play.anchor,
                FactAnchor::VideoClock(VideoClock {
                    elapsed_seconds: 70.0
                })
            );
        }
        other => panic!("Play を期待したが {other:?}"),
    }
}

#[test]
fn 移行計画は_stoppage_の_end_match_clock_を_start_と同値にする() {
    let phase = phase_start(0.0, 1800.0);
    let pause = MatchFact {
        id: FactId(Uuid::from_u128(150)),
        recorded_at: chrono::DateTime::from_timestamp(5, 0).expect("固定秒は有効"),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind: StoppageKind::Timeout,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: 300.0,
            }),
            end_anchor: None,
            note: None,
        })),
    };
    let plan = video_migration_plan(
        &[phase.clone(), pause.clone()],
        &[sync(&phase, 10.0, 1810.0)],
        &[sync(&pause, 400.0, 460.0)],
    )
    .expect("計画成立");

    match &plan[1].payload {
        MatchFactPayload::Control(ControlFact::Stoppage(payload)) => {
            assert_eq!(
                payload.end_anchor,
                Some(FactAnchor::Both {
                    // Stoppage 中に matchClock は進まない — end の matchClock は start と同値。
                    match_clock: MatchClock {
                        elapsed_seconds: 300.0
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: 460.0
                    },
                })
            );
        }
        other => panic!("Stoppage を期待したが {other:?}"),
    }
}

#[test]
fn 移行計画は_sync_欠落を拒否する() {
    let phase = phase_start(0.0, 1800.0);
    let result = video_migration_plan(std::slice::from_ref(&phase), &[], &[]);
    assert_eq!(
        result,
        Err(VideoMigrationPlanError::MissingPhaseSync { fact_id: phase.id })
    );
}

#[test]
fn 移行計画は_video_anchor_済み_play_を触らない() {
    let phase = phase_start(0.0, 1800.0);
    let mut converted = goal_at(0.0);
    if let MatchFactPayload::Play(play) = &mut converted.payload {
        play.anchor = FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 99.0,
        });
    }
    let plan = video_migration_plan(
        &[phase.clone(), converted.clone()],
        &[sync(&phase, 10.0, 1810.0)],
        &[],
    )
    .expect("計画成立");
    match &plan[1].payload {
        MatchFactPayload::Play(play) => {
            assert_eq!(
                play.anchor,
                FactAnchor::VideoClock(VideoClock {
                    elapsed_seconds: 99.0
                })
            );
        }
        other => panic!("Play を期待したが {other:?}"),
    }
}

// ── 動画ソースの差し替え計画（handball-project#267）──

fn youtube(id: &str) -> VideoSource {
    VideoSource {
        provider: VideoProvider::Youtube,
        external_id: id.to_string(),
    }
}

fn local(id: &str) -> VideoSource {
    VideoSource {
        provider: VideoProvider::Local,
        external_id: id.to_string(),
    }
}

#[test]
fn video_試合は_variant_を保って動画ソースだけ差し替わる() {
    let plan = video_source_replacement_plan(
        &MatchConfiguration::Video(youtube("fgRWI6C3UZM")),
        local("ASSET/L0/001"),
    )
    .expect("計画成立");
    assert_eq!(plan, MatchConfiguration::Video(local("ASSET/L0/001")));
}

#[test]
fn video_highlight_は_video_へ格下げされない() {
    let plan = video_source_replacement_plan(
        &MatchConfiguration::VideoHighlight(youtube("z5KrsvC6VAA")),
        local("ASSET/L0/002"),
    )
    .expect("計画成立");
    assert_eq!(
        plan,
        MatchConfiguration::VideoHighlight(local("ASSET/L0/002")),
        "ハイライト集をフル試合へ変える操作ではない"
    );
}

#[test]
fn ローカルから_youtube_へも戻せる() {
    let plan = video_source_replacement_plan(
        &MatchConfiguration::Video(local("ASSET/L0/001")),
        youtube("fgRWI6C3UZM"),
    )
    .expect("計画成立");
    assert_eq!(plan, MatchConfiguration::Video(youtube("fgRWI6C3UZM")));
}

#[test]
fn timer_試合の差し替えは拒否する() {
    let result = video_source_replacement_plan(
        &MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        youtube("fgRWI6C3UZM"),
    );
    assert_eq!(
        result,
        Err(VideoSourceReplacementError::SourceConfigurationHasNoVideo {
            kind: MatchConfigurationKind::Timer
        }),
        "タイマー → 動画は同期点が要る（video_migration_plan の仕事）"
    );
}

// ── 可変長 phase の自動補完（handball-project#352）──

#[test]
fn 可変長の前半でも隙間なく後半を補完する() {
    // 前半を 25 分に縮めた試合。番号 × 規定長で補完すると [30:00, 60:00] を作って
    // 25:00〜30:00 に隙間が開き、連続性の検証で保存ごと拒否される。
    let existing = vec![phase_start(0.0, 1500.0)];
    let plan = phase_completion_plan(&timer_match(1800.0), &existing, &goal_at(1900.0));
    assert_eq!(slots(&plan), vec![(1500.0, 3000.0)]);
}

#[test]
fn 補完する_phase_の長さは直前_phase_を踏襲する() {
    // 直前が 25 分なら補完も 25 分。明示的に phase を開始する経路（PhaseDefaults）と
    // 同じ規則で、どちらを通ったかで長さが変わらないようにしてある。
    let existing = vec![phase_start(0.0, 1500.0)];
    let plan = phase_completion_plan(&timer_match(1800.0), &existing, &goal_at(4000.0));
    assert_eq!(slots(&plan), vec![(1500.0, 3000.0), (3000.0, 4500.0)]);
}

#[test]
fn 既存_phase_が無ければ試合設定の規定長で補完する() {
    let plan = phase_completion_plan(&timer_match(1500.0), &[], &goal_at(1600.0));
    assert_eq!(slots(&plan), vec![(0.0, 1500.0), (1500.0, 3000.0)]);
}

// ── phase の長さ編集（handball-project#352）──
//
// 規則は 3 つ。開始は動かさない / 元の終了以降はすべて同じ量だけずらす /
// 短縮ではみ出す記録は新しい終了へ寄せて件数を返す。
// 「後半 7:00 で記録したものは後半 7:00 のまま」を保つのが 2 番目の規則の目的。

/// 前半 [0, 30:00] / 後半 [30:00, 60:00] の 2 phase を持つタイマー試合の fact 列。
fn 二_phase_の試合() -> Vec<MatchFact> {
    vec![phase_start(0.0, 1800.0), phase_start(1800.0, 3600.0)]
}

fn goal_with_id(id: u128, seconds: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: chrono::DateTime::from_timestamp(10, 0).expect("固定秒は有効"),
        payload: MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: None,
            player_id: Some(PlayerId(Uuid::from_u128(50))),
            related_player_id: None,
            anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: seconds,
            }),
            title: None,
            note: None,
        }),
    }
}

/// 計画の結果を「id → (開始秒, 終了秒 or None)」で読み出す（比較を読みやすくするため）。
fn 書き換え後(plan: &handball_toolkit::write::PhaseDurationChangePlan) -> BTreeMap<u128, f64> {
    plan.updated_facts
        .iter()
        .map(|fact| {
            (
                fact.id.0.as_u128(),
                fact.anchor().match_elapsed_seconds().expect("matchClock"),
            )
        })
        .collect()
}

fn 終了秒(plan: &handball_toolkit::write::PhaseDurationChangePlan, id: u128) -> f64 {
    plan.updated_facts
        .iter()
        .find(|fact| fact.id.0.as_u128() == id)
        .and_then(|fact| match &fact.payload {
            MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => {
                payload.end_anchor.match_elapsed_seconds()
            }
            _ => None,
        })
        .expect("対象 id の PhaseStart が計画に載っている")
}

#[test]
fn 前半を縮めると後半が同じ量だけ前へ動く() {
    let facts = 二_phase_の試合();
    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect("タイマーモードの regular phase なら計画できる");

    // 前半は開始 0 のまま終了だけ 25:00 へ。後半は開始 25:00 / 終了 55:00（長さ 30 分を保つ）。
    assert_eq!(終了秒(&plan, 200), 1500.0);
    assert_eq!(書き換え後(&plan).get(&2000), Some(&1500.0));
    assert_eq!(終了秒(&plan, 2000), 3300.0);
    assert!(plan.clamped_fact_ids.is_empty());
}

#[test]
fn 後半の記録は_phase_内の位置を保つ() {
    let mut facts = 二_phase_の試合();
    // 後半 2:00 の得点（累積 32:00 として保存されている）。
    facts.push(goal_with_id(300, 1920.0));

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect("計画できる");

    // 後半の開始が 5 分前へ動くので、得点も 5 分前へ動いて「後半 2:00」のまま。
    assert_eq!(書き換え後(&plan).get(&300), Some(&1620.0));
    assert!(plan.clamped_fact_ids.is_empty());
}

#[test]
fn 前半の記録は動かない() {
    let mut facts = 二_phase_の試合();
    facts.push(goal_with_id(300, 600.0));

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect("計画できる");

    assert_eq!(書き換え後(&plan).get(&300), None);
}

#[test]
fn 新しい終了を超える記録は終了へ寄せて件数に載せる() {
    let mut facts = 二_phase_の試合();
    // 前半 27:00 と 28:30 の得点。25 分へ縮めると 2 件とも枠の外へ出る。
    facts.push(goal_with_id(300, 1620.0));
    facts.push(goal_with_id(301, 1710.0));

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect("計画できる");

    let 結果 = 書き換え後(&plan);
    assert_eq!(結果.get(&300), Some(&1500.0));
    assert_eq!(結果.get(&301), Some(&1500.0));
    assert_eq!(
        plan.clamped_fact_ids,
        vec![FactId(Uuid::from_u128(300)), FactId(Uuid::from_u128(301))]
    );
}

#[test]
fn 新しい終了ちょうどの記録は寄せない() {
    let mut facts = 二_phase_の試合();
    facts.push(goal_with_id(300, 1500.0));

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect("計画できる");

    assert_eq!(書き換え後(&plan).get(&300), None);
    assert!(plan.clamped_fact_ids.is_empty());
}

#[test]
fn 伸ばす場合は寄せる記録が出ない() {
    let mut facts = 二_phase_の試合();
    facts.push(goal_with_id(300, 1620.0));
    facts.push(goal_with_id(301, 1920.0));

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        2100.0,
    )
    .expect("計画できる");

    let 結果 = 書き換え後(&plan);
    // 前半 27:00 は前半のまま動かない。後半 2:00 は後半 2:00 のまま 5 分後ろへ。
    assert_eq!(結果.get(&300), None);
    assert_eq!(結果.get(&301), Some(&2220.0));
    assert_eq!(結果.get(&2000), Some(&2100.0));
    assert!(plan.clamped_fact_ids.is_empty());
}

#[test]
fn 最後の_phase_より後ろの記録も同じ量だけ動く() {
    let mut facts = 二_phase_の試合();
    // どの phase にも属さない記録（タイマーモードでは枠外の記録も保存できる）。
    facts.push(goal_with_id(300, 3700.0));

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect("計画できる");

    assert_eq!(書き換え後(&plan).get(&300), Some(&3400.0));
}

#[test]
fn 中断の_marker_も同じ規則で動く() {
    let mut facts = 二_phase_の試合();
    facts.push(MatchFact {
        id: FactId(Uuid::from_u128(400)),
        recorded_at: chrono::DateTime::from_timestamp(11, 0).expect("固定秒は有効"),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind: StoppageKind::Timeout,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: 2000.0,
            }),
            end_anchor: None,
            note: None,
        })),
    });

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect("計画できる");

    assert_eq!(書き換え後(&plan).get(&400), Some(&1700.0));
}

#[test]
fn 計画は_phase_の鎖を先に記録を後に並べる() {
    // 発火は逐次・非 atomic。鎖が半分だけ書き換わると連続性違反でその試合へ何も
    // 保存できなくなるので、危険な窓を先頭の数件へ寄せてある。
    let mut facts = 二_phase_の試合();
    facts.push(goal_with_id(300, 600.0));
    facts.push(goal_with_id(301, 1920.0));

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect("計画できる");

    let ids: Vec<u128> = plan
        .updated_facts
        .iter()
        .map(|fact| fact.id.0.as_u128())
        .collect();
    // 前半 (200) → 後半 (2000) → 後半の得点 (301)。前半の得点 (300) は動かないので載らない。
    assert_eq!(ids, vec![200, 2000, 301]);
}

#[test]
fn 長さが変わらなければ書き換えない() {
    let mut facts = 二_phase_の試合();
    facts.push(goal_with_id(300, 1920.0));

    let plan = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(200)),
        1800.0,
    )
    .expect("計画できる");

    assert!(plan.updated_facts.is_empty());
    assert!(plan.clamped_fact_ids.is_empty());
}

#[test]
fn 動画モードの_phase_は長さ編集の対象外() {
    let match_ = Match {
        configuration: MatchConfiguration::Video(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "abc".to_string(),
        }),
        ..timer_match(1800.0)
    };
    let error = phase_duration_change_plan(
        &match_,
        &二_phase_の試合(),
        FactId(Uuid::from_u128(200)),
        1500.0,
    )
    .expect_err("動画モードは拒否される");

    assert_eq!(
        error,
        PhaseDurationChangeError::NotTimerConfiguration {
            kind: MatchConfigurationKind::Video
        }
    );
}

#[test]
fn 長さが_0_以下なら拒否する() {
    let error = phase_duration_change_plan(
        &timer_match(1800.0),
        &二_phase_の試合(),
        FactId(Uuid::from_u128(200)),
        0.0,
    )
    .expect_err("0 秒の phase は作れない");

    assert_eq!(
        error,
        PhaseDurationChangeError::InvalidDuration { seconds: 0.0 }
    );
}

#[test]
fn 対象が_phase_start_でなければ拒否する() {
    let mut facts = 二_phase_の試合();
    facts.push(goal_with_id(300, 600.0));

    let error = phase_duration_change_plan(
        &timer_match(1800.0),
        &facts,
        FactId(Uuid::from_u128(300)),
        1500.0,
    )
    .expect_err("play fact に長さは無い");

    assert_eq!(
        error,
        PhaseDurationChangeError::NotPhaseStartFact {
            fact_id: FactId(Uuid::from_u128(300))
        }
    );
}

#[test]
fn 対象_id_が無ければ拒否する() {
    let error = phase_duration_change_plan(
        &timer_match(1800.0),
        &二_phase_の試合(),
        FactId(Uuid::from_u128(999)),
        1500.0,
    )
    .expect_err("存在しない id");

    assert_eq!(
        error,
        PhaseDurationChangeError::PhaseFactNotFound {
            fact_id: FactId(Uuid::from_u128(999))
        }
    );
}
