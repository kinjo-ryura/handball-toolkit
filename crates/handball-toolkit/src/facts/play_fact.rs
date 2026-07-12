//! 移植元: `Facts/PlayFact.swift`。

use serde::{Deserialize, Serialize};

use crate::clock::FactAnchor;
use crate::ids::{PlayerId, TeamId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayEventKind {
    Goal,
    ShotMissed,
    FreeNote,
    YellowCard,
    TwoMinuteSuspension,
    RedCard,
}

impl PlayEventKind {
    /// Swift `CaseIterable.allCases` 相当。
    pub const ALL_CASES: [PlayEventKind; 6] = [
        PlayEventKind::Goal,
        PlayEventKind::ShotMissed,
        PlayEventKind::FreeNote,
        PlayEventKind::YellowCard,
        PlayEventKind::TwoMinuteSuspension,
        PlayEventKind::RedCard,
    ];
}

/// 1 件のプレイ事実。
///
/// `phase` フィールドは持たない（PhaseStart fact の range から projection で逆引き）。
/// `team_id` は全 kind で optional。ただし score 系 projection は team_id を直接参照し
/// player からの導出 fallback を持たないため、goal / shotMissed では実質必須（UI 側で担保）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayFact {
    pub kind: PlayEventKind,
    pub team_id: Option<TeamId>,
    pub player_id: Option<PlayerId>,
    pub related_player_id: Option<PlayerId>,
    pub anchor: FactAnchor,
    pub title: Option<String>,
    pub note: Option<String>,
}
