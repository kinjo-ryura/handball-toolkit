//! 永続化順（累積秒 → recordedAt → id）の規約そのものを固定する（handball-project#87）。
//!
//! これまでこの規約は import 経路の `commit_plan_sorts_facts_into_persistence_order` からしか
//! 触れられておらず、規約単体の回帰ロックが無かった。オラクルは読み出し側の
//! `SwiftDataMatchRepository.factRecordOrder`。

use chrono::{DateTime, Utc};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::facts::{MatchFact, MatchFactPayload, PlayEventKind, PlayFact};
use handball_toolkit::ids::{FactId, PlayerId, TeamId};
use handball_toolkit::persistence_order::{persistence_ordered, sort_by_persistence_order};
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
fn both_anchor_は_match_clock_を優先する() {
    // video 秒だけ見ると 5 → 100 の順になるが、match 秒（100 → 200）が優先されるべき。
    let mut facts = vec![
        fact(2, 0, both_anchor(200.0, 5.0)),
        fact(1, 0, both_anchor(100.0, 100.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![1, 2]);
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
fn キーは_3_段で優先順位どおりに効く() {
    // 秒が最優先。秒が同じものの中でだけ recordedAt、さらに同じものの中でだけ id。
    let mut facts = vec![
        fact(2, 500, match_anchor(600.0)),
        fact(9, 100, match_anchor(1800.0)),
        fact(1, 500, match_anchor(600.0)),
        fact(5, 100, match_anchor(600.0)),
    ];
    sort_by_persistence_order(&mut facts);
    assert_eq!(ids(&facts), vec![5, 1, 2, 9]);
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
