//! 計画層 `write`（純粋関数）の挙動固定（ADR 0005 実装順序 1・3）。
//!
//! - roster 構築の後方互換ルールを移植元 `SwiftDataMatchRepository.loadRosterContext` と
//!   同セマンティクスで固定する: 選手 0 件は None（参照整合 skip）・同一選手の重複は先勝ち
//! - phase 自動補完計画を移植元 `RecordingScreenStore.ensureTimerPhasesCovering`
//!   （とその store テスト群）と同セマンティクスで固定する

use std::collections::{BTreeMap, BTreeSet};

use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
use handball_toolkit::write::{
    NewFactStamp, PlayerTeamRef, phase_completion_fact, phase_completion_plan,
    roster_context_from_players,
};
use uuid::Uuid;

#[test]
fn 選手_0_件なら_none_で参照整合を_skip_する() {
    let home = Uuid::from_u128(1);
    let away = Uuid::from_u128(2);
    assert_eq!(roster_context_from_players(home, away, &[]), None);
}

#[test]
fn 選手一覧から_lookup_と_known_ids_を組む() {
    let home = Uuid::from_u128(1);
    let away = Uuid::from_u128(2);
    let p1 = Uuid::from_u128(11);
    let p2 = Uuid::from_u128(12);
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
    let home = Uuid::from_u128(1);
    let away = Uuid::from_u128(2);
    let p1 = Uuid::from_u128(11);
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
        id: Uuid::from_u128(1),
        title: None,
        date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
        home_team_id: Uuid::from_u128(2),
        away_team_id: Uuid::from_u128(3),
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: duration,
        },
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    }
}

fn goal_at(seconds: f64) -> MatchFact {
    MatchFact {
        id: Uuid::from_u128(100),
        recorded_at: chrono::DateTime::from_timestamp(10, 0).expect("固定秒は有効"),
        payload: MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: None,
            player_id: Some(Uuid::from_u128(50)),
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
        id: Uuid::from_u128((start as u128) + 200),
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
        id: Uuid::from_u128(101),
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
        id: Uuid::from_u128(77),
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
