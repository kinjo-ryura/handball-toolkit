//! 移植元: `Validation/DomainValidationIssue.swift`。

use serde::{Deserialize, Serialize};

use super::{
    ConfigurationValidationError, FactValidationError, MatchValidationError,
    TimelineValidationError,
};

/// 4 系統の validation error を直接運ぶ sum type。
///
/// severity を持たない（一律 blocking）:
/// - 自動修復は domain layer で行わない（validation は判定のみ、修復はシェルの責務）
/// - 文脈（タイマーモード / 動画モード等）は validation ルール側で吸収
/// - 旧 `ValidationSeverity` enum / severity 付き struct ラッパーは廃止
///
/// 将来 `warning` が必要になれば、その時点で struct ラッパーへ切り替える。
///
/// serde 形式がそのまま境界のワイヤ形式（ADR 0002）:
/// variant tag が `scope`、内側のエラーが `code` + `params` を与え、全体で
/// `{ "scope": "fact", "code": "negativeMatchClock", "params": {...} }` になる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "scope", rename_all = "camelCase")]
pub enum DomainValidationIssue {
    Match(MatchValidationError),
    Configuration(ConfigurationValidationError),
    Fact(FactValidationError),
    Timeline(TimelineValidationError),
}
