//! 移植元: `Entities/Player.swift`。

use serde::{Deserialize, Serialize};

use crate::ids::{PlayerId, TeamId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub id: PlayerId,
    pub team_id: TeamId,
    pub name: String,
    pub jersey_number: Option<i64>,
    pub photo: Option<PlayerPhoto>,
}

/// 写真本体は domain では持たず、storage への参照だけを保持する。
/// 実ファイルの lifecycle は infrastructure 側の `ImageStore` が管理する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PlayerPhoto {
    pub storage_key: String,
}
