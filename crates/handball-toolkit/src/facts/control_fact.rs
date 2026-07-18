//! 移植元: `Facts/ControlFact.swift`。

use serde::{Deserialize, Serialize};

use crate::clock::FactAnchor;
use crate::configuration::PhaseKind;

/// 試合進行に関する制御イベント。
///
/// 旧 `ControlEventKind` flat enum (6 種: phaseStarted / phaseEnded / timeoutStarted / timeoutEnded / paused / resumed)
/// を sum type 2 variant に集約:
/// - `PhaseStart`: phase の開始 + 終了範囲を 1 fact で保持（end は常に値あり）
/// - `Stoppage`: 試合タイマーが止まる区間を 1 fact で保持（start + optional end）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum ControlFact {
    PhaseStart(PhaseStartPayload),
    Stoppage(StoppagePayload),
}

/// PhaseStart の payload。
///
/// `end_anchor` は常に値あり（生成時に必ずユーザー入力ダイアログを経由する不変条件）。
/// 規定長は `end_anchor.elapsed_seconds - start_anchor.elapsed_seconds` で導出。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PhaseStartPayload {
    pub kind: PhaseKind,
    pub start_anchor: FactAnchor,
    pub end_anchor: FactAnchor,
}

/// Stoppage (timeout / pause) の payload。
///
/// `end_anchor` は `Timer` mode では常に None、`Video` mode では値あり。
/// 開始のみ記録するモード（タイマーモード）と、区間として記録するモード（動画モード）の両対応。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct StoppagePayload {
    pub kind: StoppageKind,
    pub start_anchor: FactAnchor,
    // uniffi(default) は移植元 Swift init のデフォルト引数の保存。
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub end_anchor: Option<FactAnchor>,
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum StoppageKind {
    /// 戦術的タイムアウト。
    Timeout,
    /// それ以外の中断（怪我 / VAR / 主審判定など）。
    Pause,
}

impl StoppageKind {
    /// Swift `CaseIterable.allCases` 相当。
    pub const ALL_CASES: [StoppageKind; 2] = [StoppageKind::Timeout, StoppageKind::Pause];
}

impl ControlFact {
    /// payload を問わず開始 anchor を返す。
    pub fn start_anchor(&self) -> FactAnchor {
        match self {
            ControlFact::PhaseStart(payload) => payload.start_anchor,
            ControlFact::Stoppage(payload) => payload.start_anchor,
        }
    }

    /// payload を問わず終了 anchor を返す（PhaseStart は常に値あり、Stoppage は optional）。
    pub fn end_anchor(&self) -> Option<FactAnchor> {
        match self {
            ControlFact::PhaseStart(payload) => Some(payload.end_anchor),
            ControlFact::Stoppage(payload) => payload.end_anchor,
        }
    }
}
