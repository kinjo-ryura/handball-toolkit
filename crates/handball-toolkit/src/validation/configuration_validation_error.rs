//! 移植元: `Validation/ConfigurationValidationError.swift`。

use serde::{Deserialize, Serialize};

/// MatchConfiguration 単体に対する validation error。
///
/// MatchConfiguration が sum type 化したことで、多くの旧 case が型レベルで作れず error 不要に:
/// - `noPhasesDefined` / `duplicatePhaseDefinitions`（PhaseRules 廃止）
/// - `emptyPhaseLabel` / `nonPositiveNominalDuration` / `missingNominalDuration`（PhaseRules 廃止）
/// - `fullMatchContainsTimelinePhase` 系（ContentKind 廃止、PhaseRules 廃止）
/// - `highlightsMustUseSingleTimelinePhase` 系（sum type 化で `VideoHighlight(VS)` は VS 必須）
/// - `videoCaptureRequiresVideoSource`（sum type 化で `Video(VS)` は VS 必須）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(
    tag = "code",
    content = "params",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConfigurationValidationError {
    /// `Timer { phase_duration_seconds }` payload > 0 必須。0 秒・負数は不正。
    NonPositivePhaseDuration { seconds: f64 },
    /// `Video(VS)` / `VideoHighlight(VS)` の `VS.external_id` trim 非空必須。
    ///
    /// code は Swift case 名そのまま（`ID` 大文字。ADR 0002 の安定契約）のため明示 rename。
    #[serde(rename = "emptyVideoExternalID")]
    EmptyVideoExternalId,
}
