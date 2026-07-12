//! 移植元: `Validation/TimelineValidationError.swift`。

use serde::{Deserialize, Serialize};

use crate::configuration::PhaseKind;

/// fact log 全体としての順序・整合性 validation error。
///
/// 旧 enum から削除された case:
/// - `duplicatePhaseEnd` / `phaseEndWithoutStart`（`phaseEnded` fact 廃止）
/// - `playRecordedBeforePhaseStart`（R7 / R8 に統合）
/// - `secondHalfStartedBeforeFirstHalfStart` 等（`invalidPhaseOrder` 系廃止、順序 skip 許容）
/// - `timeoutStartedWhileAnotherStopIsOpen` 等（Stoppage 1 fact 化、`stoppagesOverlap` に統合）
/// - `incompleteStoppedIntervalInCompleteMode` 等（`complete` mode 廃止）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "code",
    content = "params",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TimelineValidationError {
    // ── Configuration × PhaseStart 整合 (R3 / R5 / R6) ──
    /// R3 (改): `Timer + fact 1 件以上 + PhaseStart fact なし`。
    TimerWithFactsMissingPhaseStart,
    /// R5 (新): `Video + fact 1 件以上 + PhaseStart fact なし`。
    VideoWithFactsMissingPhaseStart,
    /// R6 (新): `VideoHighlight + PhaseStart fact あり`（ハイライトに phase 概念は矛盾）。
    VideoHighlightContainsPhaseStart,

    // ── Configuration × Stoppage 整合 (R9) ──
    /// R9 (新): `VideoHighlight + Stoppage fact あり`（ハイライトに Stoppage 概念は矛盾）。
    VideoHighlightContainsStoppage,

    // ── Configuration × title 整合 (R11) ──
    /// R11 (新): `VideoHighlight + Match.title が None または trim 後空文字`。
    /// ハイライトは内容識別のため title 必須（= 同試合の複数ハイライト「ゴール集」「守備集」等を区別）。
    VideoHighlightMissingTitle,

    // ── Play fact anchor 範囲 (R7 / R8) ──
    /// R7 (新): `Video + play fact anchor が PhaseStart fact range の外`（ハーフタイム中 record 禁止）。
    /// `kind` は隣接 phase の hint。判定不能な場合は None。
    PlayRecordedOutsidePhaseRange { kind: Option<PhaseKind> },
    /// R8 (新): `Video` / `Timer` の Stoppage range 内に play fact anchor（試合停止中 record 禁止）。
    PlayRecordedInsideStoppage,

    // ── Phase 順序 / 連続性 ──
    /// `Shootout` PhaseStart fact が複数。shootout は試合で 1 件まで。
    DuplicateShootout,
    /// shootout PhaseStart の後に regular PhaseStart fact がある。shootout は最後でなければならない。
    ShootoutNotLast,
    /// `Timer` の regular phase の startAnchor が直前 regular PhaseStart fact の endAnchor と等しくない。
    /// matchClock 累積秒の連続性を保つ。overlap も gap も blocking。
    /// `Video` は SegmentResolver の baseline rolling forward で構造的に保証されるため対象外。
    PhaseStartNotContinuousFromPrevious,

    // ── Stoppage 重複 / phase 外 ──
    /// Stoppage 同士の range overlap 禁止（stopped 区間のネスト禁止）。
    StoppagesOverlap,
    /// Stoppage は PhaseStart fact range 内（ハーフタイム中の Stoppage 禁止）。
    StoppageOutsidePhaseRange,
}
