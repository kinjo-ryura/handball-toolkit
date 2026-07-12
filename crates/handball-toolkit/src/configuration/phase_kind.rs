//! 移植元: `Configuration/PhaseKind.swift`。

use serde::{Deserialize, Serialize};

/// Phase の種類。
/// - `Regular`: タイマーが動く phase（前半・後半・延長前半・延長後半など）
/// - `Shootout`: タイマーが動かない（matchClock 累積秒は phase 開始時点で固定）
///
/// 旧 `MatchPhase` enum (firstHalf / secondHalf / overtime1 / overtime2 / shootout) を置き換え。
/// 役割名は UI 層が出現順から導出（"phase 1" / "phase 2" / ...）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PhaseKind {
    Regular,
    Shootout,
}

impl PhaseKind {
    /// Swift `CaseIterable.allCases` 相当。
    pub const ALL_CASES: [PhaseKind; 2] = [PhaseKind::Regular, PhaseKind::Shootout];
}
