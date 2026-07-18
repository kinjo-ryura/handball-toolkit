//! 移植元: `Entities/Match.swift`。

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::configuration::MatchConfiguration;
use crate::ids::{MatchId, PlayerId, TeamId};

/// ロースター選択。Swift の `Set<PlayerID>` は決定性（エラー payload・ゴールデン出力の順序）
/// のため `BTreeSet` で移植する（ADR 0001。集合演算の意味論は同一）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct RosterSelection {
    pub benched_player_ids: BTreeSet<PlayerId>,
    pub out_of_roster_player_ids: BTreeSet<PlayerId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub id: MatchId,
    pub title: Option<String>,
    pub date: DateTime<Utc>,
    pub home_team_id: TeamId,
    pub away_team_id: TeamId,
    pub configuration: MatchConfiguration,
    pub roster_selection: RosterSelection,
    /// スコア / イベント一覧で「ホームを左」に表示するかどうか。
    /// コートの実配置に合わせて per-match で切り替える前提（V1 DisplaySettingsSheet 同等）。
    /// default true で legacy 互換を保つ。
    pub is_home_on_left: bool,
}
