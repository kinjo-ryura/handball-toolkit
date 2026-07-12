//! 移植元: `Facts/MatchFact.swift`。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::clock::FactAnchor;
use crate::ids::FactId;

use super::{ControlFact, PlayFact};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchFactPayload {
    Play(PlayFact),
    Control(ControlFact),
}

/// 永続化対象の「事実」1 件。id / timestamp はここに一元化。
/// `recorded_at` は整列 tie-break 専用（位置づけは anchor — ADR 0001）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchFact {
    pub id: FactId,
    pub recorded_at: DateTime<Utc>,
    pub payload: MatchFactPayload,
}

impl MatchFact {
    /// payload を問わず代表 anchor を返す。
    /// PlayFact は唯一の anchor、ControlFact は startAnchor を返す。
    pub fn anchor(&self) -> FactAnchor {
        match &self.payload {
            MatchFactPayload::Play(play) => play.anchor,
            MatchFactPayload::Control(control) => control.start_anchor(),
        }
    }
}
