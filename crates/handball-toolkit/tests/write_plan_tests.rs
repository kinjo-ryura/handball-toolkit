//! 計画層 `write`（純粋関数）の挙動固定（ADR 0005 実装順序 1）。
//!
//! roster 構築の後方互換ルールを移植元 `SwiftDataMatchRepository.loadRosterContext` と
//! 同セマンティクスで固定する: 選手 0 件は None（参照整合 skip）・同一選手の重複は先勝ち。

use std::collections::{BTreeMap, BTreeSet};

use handball_toolkit::write::{PlayerTeamRef, roster_context_from_players};
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
