//! 書き込み経路の計画層（純粋関数・feature 非依存 — ADR 0005 決定 1）。
//!
//! 「検証入力をどう組むか・何をどの順に保存すべきか」の判断を、fact 列 in → 導出結果 out の
//! 純粋関数として置く。永続化の発火（repository を await する orchestration）は
//! feature `uniffi` 配下の `ffi_write` が担う。

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{PlayerId, TeamId};
use crate::validators::RosterContext;

/// home / away 所属選手 1 件の (player, team) 参照。
/// `MatchWriteRepository::load_roster_players` が返す roster 構築材料。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PlayerTeamRef {
    pub player_id: PlayerId,
    pub team_id: TeamId,
}

/// 所属選手一覧から validation 用の `RosterContext` を組む。
///
/// 0 件なら `None` = 参照整合チェックを skip する後方互換ルール（移植元:
/// `SwiftDataMatchRepository.loadRosterContext` の `guard !players.isEmpty else { return nil }`。
/// この判断はシェルからコアへ移した — ADR 0005 決定 1）。
/// 同一 player の重複は先勝ち（移植元 `uniquingKeysWith: { first, _ in first }` と同じ）。
pub fn roster_context_from_players(
    home_team_id: TeamId,
    away_team_id: TeamId,
    players: &[PlayerTeamRef],
) -> Option<RosterContext> {
    if players.is_empty() {
        return None;
    }
    let mut player_team_lookup = BTreeMap::new();
    let mut known_player_ids = BTreeSet::new();
    for player in players {
        player_team_lookup
            .entry(player.player_id)
            .or_insert(player.team_id);
        known_player_ids.insert(player.player_id);
    }
    Some(RosterContext {
        home_team_id,
        away_team_id,
        player_team_lookup,
        known_player_ids: Some(known_player_ids),
    })
}
