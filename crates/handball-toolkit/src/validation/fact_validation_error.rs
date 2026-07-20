//! 移植元: `Validation/FactValidationError.swift`。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::clock::FactAnchorKind;
use crate::configuration::MatchConfigurationKind;
use crate::facts::{PlayEventKind, StoppageKind};
use crate::ids::{PlayerId, TeamId};

/// 1 件の fact（Play / Control）単体に対する validation error。
///
/// 旧 enum から削除された case（新原則適用により消失）:
/// - `anchorPhaseMismatch`（`MatchClock` から phase 削除に伴い消失）
/// - `phaseNotDefinedInConfiguration`（PhaseRules 廃止）
/// - `highlightsPlayMustUseTimeline`（`VideoHighlight` は phase 概念無し）
/// - `highlightsControlKindNotAllowed`（R6 / R9 に統合）
/// - `videoControlRequiresBothClocks`（`Both` 必須廃止）
/// - `missingTeamForPlayKind`（`team_id` 全 Kind optional 化）
/// - `playerRequiresTeamReference` / `relatedPlayerRequiresTeamReference`（`team_id` optional 化）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(
    tag = "code",
    content = "params",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FactValidationError {
    // ── anchor 系 ──
    NegativeMatchClock,
    NegativeVideoClock,
    /// 秒値が非有限（NaN / ±∞）。移植元 Swift には無い、Rust 側で新設した case。
    ///
    /// `NaN < 0.0` は false になるため負値検査を素通りする。素通りした非有限値は
    /// serde_json が `null` として書き出す（Err にも panic にもならない）ので、
    /// export は成功として読み戻せない JSON を書く。projection 側でも NaN は比較が
    /// 常に false になり `SegmentResolver` の区間判定にヒットせず、phase 別集計から
    /// 黙って除外される。いずれも失敗が書き込み時点に現れないため、ここで弾く。
    /// 詳細は handball-project#91。
    NonFiniteMatchClock,
    /// 秒値が非有限（NaN / ±∞）。詳細は [`FactValidationError::NonFiniteMatchClock`]。
    NonFiniteVideoClock,
    InvalidAnchorForConfiguration {
        configuration: MatchConfigurationKind,
        actual: FactAnchorKind,
        allowed: BTreeSet<FactAnchorKind>,
    },

    // ── 共通文字列 / 参照系 ──
    EmptyTitle,
    EmptyNote,
    DuplicatePrimaryAndRelatedPlayer,

    // ── PlayFact kind 必須項目 ──
    MissingPlayerForPlayKind {
        kind: PlayEventKind,
    },
    FreeNoteHasNoContent,

    // ── PhaseStart 必須項目 / range ──
    /// PhaseStart の `end_anchor` 必須（生成時に必ず入力ダイアログを経由する不変条件）。
    /// 型上は non-optional のため通常起きないが、JSON decode / migration 由来データで発生しうる。
    PhaseStartMissingEndAnchor,
    /// startAnchor と endAnchor の anchor kind が異なる（例: start は videoClock、end は matchClock）。
    PhaseStartAnchorMismatch,
    /// endAnchor.elapsedSeconds > startAnchor.elapsedSeconds でない。0 length / 逆順。
    PhaseStartEndBeforeStart,

    // ── Stoppage 必須項目 / range ──
    StoppageEndBeforeStart,
    StoppageEndNilInVideoMode {
        kind: StoppageKind,
    },
    StoppageEndPresentInTimerMode {
        kind: StoppageKind,
    },
    /// `Timeout` kind は `note == None` 必須（戦術的タイムアウトに自由記述ニーズが現状無い）。
    TimeoutHasNote,
    /// `Pause` の `note` が trim 後空文字（None は valid、空文字は None 相当）。
    EmptyStoppageNote,

    // ── team / player 参照整合 ──
    UnknownTeamReference {
        team_id: TeamId,
    },
    UnknownPlayerReference {
        player_id: PlayerId,
    },
    PlayerTeamMismatch {
        player_id: PlayerId,
        team_id: TeamId,
    },
    RelatedPlayerTeamMismatch {
        player_id: PlayerId,
        team_id: TeamId,
    },
}
