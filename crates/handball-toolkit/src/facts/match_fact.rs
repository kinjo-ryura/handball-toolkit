//! 移植元: `Facts/MatchFact.swift`。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::clock::FactAnchor;
use crate::ids::FactId;

use super::{ControlFact, PlayFact, PossessionFact};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum MatchFactPayload {
    Play(PlayFact),
    Control(ControlFact),
    /// ポゼッション開始（handball-project#154）。play / control のどちらでもない第 3 の種別。
    Possession(PossessionFact),
}

/// 永続化対象の「事実」1 件。id / timestamp はここに一元化。
/// `recorded_at` は整列 tie-break 専用（位置づけは anchor — ADR 0001）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct MatchFact {
    pub id: FactId,
    pub recorded_at: DateTime<Utc>,
    pub payload: MatchFactPayload,
}

impl MatchFact {
    /// payload を問わず代表 anchor を返す。
    /// PlayFact / PossessionFact は唯一の anchor、ControlFact は startAnchor を返す。
    pub fn anchor(&self) -> FactAnchor {
        match &self.payload {
            MatchFactPayload::Play(play) => play.anchor,
            MatchFactPayload::Control(control) => control.start_anchor(),
            MatchFactPayload::Possession(possession) => possession.anchor,
        }
    }

    /// 「点として扱う fact」の代表 anchor = 始まり。R7 / R8 のように点を対象にする箇所で使う。
    /// ControlFact は start / end の range を持つので `None`。
    ///
    /// **`PossessionFact` は任意の end を持つが、ここでは始まりだけを返す**
    /// （handball-project#220）。R7（phase range 内）/ R8（Stoppage range 外）を end にも掛けると
    /// 「end が phase 範囲内」を blocking で要求することになり、意図的に置かないと決めたルールが
    /// 裏口から復活する（`DOMAIN_VALIDATION_RULES.md`「持たないルール」）。
    pub fn single_anchor(&self) -> Option<FactAnchor> {
        match &self.payload {
            MatchFactPayload::Play(play) => Some(play.anchor),
            MatchFactPayload::Possession(possession) => Some(possession.anchor),
            MatchFactPayload::Control(_) => None,
        }
    }

    /// [`Self::single_anchor`] の書き換え版（timer → video migration の anchor 変換で使う）。
    pub fn single_anchor_mut(&mut self) -> Option<&mut FactAnchor> {
        match &mut self.payload {
            MatchFactPayload::Play(play) => Some(&mut play.anchor),
            MatchFactPayload::Possession(possession) => Some(&mut possession.anchor),
            MatchFactPayload::Control(_) => None,
        }
    }
}
