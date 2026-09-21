//! 永続化順（動画秒を持つか → 時刻 → phase 開始か → recordedAt → id）の規約そのものを固定する
//! （handball-project#87 / #401）。
//!
//! これまでこの規約は import 経路の `commit_plan_sorts_facts_into_persistence_order` からしか
//! 触れられておらず、規約単体の回帰ロックが無かった。オラクルは読み出し側の
//! `SwiftDataMatchRepository.factRecordOrder`。
//!
//! 末尾の `共通_fixture_の並びと一致する` は、同じ規約を別々に実装している Swift / Android と
//! 共有する fixture（`tests/fixtures/persistence-order-cases.json`）を読む（handball-project#405）。

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    PossessionFact, StoppageKind, StoppagePayload,
};
use handball_toolkit::ids::{FactId, PlayerId, TeamId};
use handball_toolkit::persistence_order::{persistence_ordered, sort_by_persistence_order};
use serde::Deserialize;
use uuid::Uuid;

fn at(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).expect("テスト用の timestamp は常に有効")
}

fn fact(id: u128, recorded_at: i64, anchor: FactAnchor) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: at(recorded_at),
        payload: MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: Some(TeamId(Uuid::from_u128(9001))),
            player_id: Some(PlayerId(Uuid::from_u128(9002))),
            related_player_id: None,
            anchor,
            title: None,
            note: None,
        }),
    }
}

/// PhaseStart fact。種別が並びに効くことを確かめる test だけが使う（handball-project#401）。
fn phase_start(id: u128, recorded_at: i64, start_secs: f64, end_secs: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: at(recorded_at),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor: match_anchor(start_secs),
            end_anchor: match_anchor(end_secs),
        })),
    }
}

fn match_anchor(secs: f64) -> FactAnchor {
    FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: secs,
    })
}

fn video_anchor(secs: f64) -> FactAnchor {
    FactAnchor::VideoClock(VideoClock {
        elapsed_seconds: secs,
    })
}

fn both_anchor(match_secs: f64, video_secs: f64) -> FactAnchor {
    FactAnchor::Both {
        match_clock: MatchClock {
            elapsed_seconds: match_secs,
        },
        video_clock: VideoClock {
            elapsed_seconds: video_secs,
        },
    }
}

fn ids(facts: &[MatchFact]) -> Vec<u128> {
    facts.iter().map(|f| f.id.0.as_u128()).collect()
}

#[test]
fn 累積秒の昇順で並ぶ() {
    let mut facts = vec![
        fact(3, 0, match_anchor(1800.0)),
        fact(1, 0, match_anchor(0.0)),
        fact(2, 0, match_anchor(600.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![1, 2, 3]);
}

#[test]
fn match_clock_が無い_fact_は_video_clock_がキーになる() {
    let mut facts = vec![
        fact(2, 0, video_anchor(1130.0)),
        fact(1, 0, video_anchor(1086.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![1, 2]);
}

#[test]
fn both_anchor_は_video_clock_を優先する() {
    // match 秒だけ見ると 100 → 200 の順になるが、video 秒（5 → 100）が優先されるべき
    // （handball-project#380）。
    let mut facts = vec![
        fact(1, 0, both_anchor(100.0, 100.0)),
        fact(2, 0, both_anchor(200.0, 5.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![2, 1]);
}

#[test]
fn 動画へ移行した試合は区間が交互にならず動画秒の順で並ぶ() {
    // 移行後の形: phaseStart は Both、play は VideoClock だけ。match 秒を優先すると
    // 後半の phaseStart（match 1800）が前半終盤の play（video 1850）より前に来ていた
    // （handball-project#380）。
    // 並べ方は anchor だけで決まるので、phaseStart も play fact の形で代用する。
    // 1: 前半の phaseStart / 2: 前半終盤の play / 3: 後半の phaseStart / 4: 後半の play
    let mut facts = vec![
        fact(4, 0, video_anchor(2200.0)),
        fact(3, 0, both_anchor(1800.0, 2100.0)),
        fact(2, 0, video_anchor(1850.0)),
        fact(1, 0, both_anchor(0.0, 120.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![1, 2, 3, 4]);
}

#[test]
fn 動画秒を持たない_fact_は後ろにまとまり累積秒で並ぶ() {
    // 移行が途中で止まった試合（handball-project#320）: 未同期の fact は match 秒だけを持つ。
    // match 秒 100 は video 秒 2000 より小さいが、別の時計どうしを比べず後ろへ寄せる。
    let mut facts = vec![
        fact(3, 0, match_anchor(200.0)),
        fact(1, 0, video_anchor(2000.0)),
        fact(2, 0, match_anchor(100.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![1, 2, 3]);
}

#[test]
fn 同じ時刻なら_phase_開始が記録より先に並ぶ() {
    // タイマーモードの phase は記録した瞬間に auto-create される（ADR 0001）ので、その phase の
    // 最初の記録と phase 開始は必ず同じ累積秒を持つ（handball-project#401）。
    // recorded_at は逆順にしてある — ここを recorded_at に任せていた頃は、スタンプの発行順が
    // 発火順と逆転したときに 00:00 の得点が「前半の開始」より上に並んでいた。
    let mut facts = vec![
        fact(2, 100, match_anchor(0.0)),
        phase_start(1, 200, 0.0, 1800.0),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![1, 2]);
}

#[test]
fn phase_開始が先に来るのは同じ時刻のときだけ() {
    // 種別は秒より弱い第 3 キー。前半終盤の得点（1700）は後半の開始（1800）より前のまま。
    let mut facts = vec![
        phase_start(3, 0, 1800.0, 3600.0),
        fact(2, 0, match_anchor(1700.0)),
        phase_start(1, 0, 0.0, 1800.0),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![1, 2, 3]);
}

#[test]
fn 同一秒は_recorded_at_で_tie_break_する() {
    let mut facts = vec![
        fact(1, 300, match_anchor(600.0)),
        fact(2, 100, match_anchor(600.0)),
        fact(3, 200, match_anchor(600.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![2, 3, 1]);
}

#[test]
fn 秒と_recorded_at_が同一なら_fact_id_で_tie_break_する() {
    // FactId の Ord は内包 Uuid のバイト順 = Swift uuidString 昇順と同順。
    let mut facts = vec![
        fact(30, 0, match_anchor(600.0)),
        fact(10, 0, match_anchor(600.0)),
        fact(20, 0, match_anchor(600.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![10, 20, 30]);
}

#[test]
fn キーは_5_段で優先順位どおりに効く() {
    // 動画秒を持つかが最優先（秒・recordedAt が大きくても前へ）。その中で秒、秒が同じものの
    // 中でだけ phase 開始か、さらに同じものの中でだけ recordedAt、最後に id。
    let mut facts = vec![
        fact(2, 500, match_anchor(600.0)),
        fact(9, 100, match_anchor(1800.0)),
        fact(7, 900, video_anchor(3000.0)),
        fact(1, 500, match_anchor(600.0)),
        fact(5, 100, match_anchor(600.0)),
        phase_start(8, 900, 600.0, 2400.0),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![7, 8, 5, 1, 2, 9]);
}

#[test]
fn persistence_ordered_は入力を変更せず新しい_vec_を返す() {
    let original = vec![
        fact(2, 0, match_anchor(600.0)),
        fact(1, 0, match_anchor(0.0)),
    ];
    let ordered = persistence_ordered(&original);

    assert_eq!(ids(&original), vec![2, 1], "入力は変更されない");
    assert_eq!(ids(&ordered), vec![1, 2]);
}

#[test]
fn 整列済みの列は不変に保たれる() {
    let sorted = vec![
        fact(1, 0, match_anchor(0.0)),
        fact(2, 0, match_anchor(600.0)),
        fact(3, 0, match_anchor(1800.0)),
    ];
    let once = persistence_ordered(&sorted);
    let twice = persistence_ordered(&once);

    assert_eq!(ids(&once), vec![1, 2, 3]);
    assert_eq!(ids(&twice), ids(&once), "冪等");
}

// ── 共通 fixture（handball-project#405）──
//
// 同じ規約の実装は 4 箇所ある（このモジュール / Swift の `factRecordOrder` / Android の Room の
// `ORDER BY` 2 本）。Room のクエリはコアの関数を呼べず、SwiftData もクエリの並びで読むため、
// 集約できない。代わりに 4 箇所が同じ fixture を読み、どれか 1 つだけ変えると赤くなるようにする。

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<FixtureCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCase {
    name: String,
    facts: Vec<FixtureFact>,
    expected: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureFact {
    id: Uuid,
    kind: FixtureKind,
    match_seconds: Option<f64>,
    video_seconds: Option<f64>,
    /// UNIX 秒。
    recorded_at: i64,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum FixtureKind {
    Play,
    PhaseStart,
    Stoppage,
    Possession,
}

/// fixture の時計 2 つから anchor を作る。どちらも無い fact は型で表せないので fixture に置かない。
fn fixture_anchor(match_secs: Option<f64>, video_secs: Option<f64>) -> FactAnchor {
    match (match_secs, video_secs) {
        (Some(m), Some(v)) => both_anchor(m, v),
        (Some(m), None) => match_anchor(m),
        (None, Some(v)) => video_anchor(v),
        (None, None) => panic!("fixture の fact はどちらかの時計を持つ（Rust の型で表せないため）"),
    }
}

fn fixture_fact(source: &FixtureFact) -> MatchFact {
    let anchor = fixture_anchor(source.match_seconds, source.video_seconds);
    let payload = match source.kind {
        FixtureKind::Play => MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: Some(TeamId(Uuid::from_u128(9001))),
            player_id: None,
            related_player_id: None,
            anchor,
            title: None,
            note: None,
        }),
        // 並びは開始 anchor だけで決まる。終了は開始を 1800 秒ずらしたもので埋める。
        FixtureKind::PhaseStart => {
            MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
                kind: PhaseKind::Regular,
                start_anchor: anchor,
                end_anchor: fixture_anchor(
                    source.match_seconds.map(|s| s + 1800.0),
                    source.video_seconds.map(|s| s + 1800.0),
                ),
            }))
        }
        FixtureKind::Stoppage => {
            MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
                kind: StoppageKind::Timeout,
                start_anchor: anchor,
                end_anchor: None,
                note: None,
            }))
        }
        FixtureKind::Possession => MatchFactPayload::Possession(PossessionFact {
            team_id: TeamId(Uuid::from_u128(9001)),
            anchor,
            end_anchor: None,
        }),
    };
    MatchFact {
        id: FactId(source.id),
        recorded_at: at(source.recorded_at),
        payload,
    }
}

#[test]
fn 共通_fixture_の並びと一致する() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/persistence-order-cases.json");
    let text = fs::read_to_string(&path).expect("共通 fixture を読めない");
    let fixture: Fixture = serde_json::from_str(&text).expect("共通 fixture の形が違う");
    // 空の fixture で緑にならないようにする（件数は書かない — 足すたびにずれる）。
    assert!(!fixture.cases.is_empty(), "共通 fixture にケースが無い");

    for case in &fixture.cases {
        let facts: Vec<MatchFact> = case.facts.iter().map(fixture_fact).collect();
        let actual: Vec<Uuid> = persistence_ordered(&facts)
            .iter()
            .map(|fact| fact.id.0)
            .collect();
        assert_eq!(actual, case.expected, "{}", case.name);
    }
}
