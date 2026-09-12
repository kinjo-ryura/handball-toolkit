//! 移植元: `Validators/FactValidator.swift`。
//!
//! 1 件の MatchFact（PlayFact / ControlFact / PossessionFact）の value + context validation。
//!
//! 役割:
//! - anchor 値の範囲（>= 0）チェック
//! - configuration ごとの anchor kind 整合
//! - PhaseStart / Stoppage の payload 整合
//! - PlayFact の kind 必須項目チェック
//! - team / player 参照整合（RosterContext を渡す場合のみ）
//!
//! fact log 全体としての順序・整合性チェックは `fact_log_validator` の責務。

use std::collections::{BTreeMap, BTreeSet};

use crate::clock::{FactAnchor, FactAnchorKind};
use crate::configuration::MatchConfiguration;
use crate::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    PossessionFact, StoppageKind, StoppagePayload,
};
use crate::ids::{PlayerId, TeamId};
use crate::validation::{DomainValidationIssue, FactValidationError};

/// player↔team の整合を見るために必要なコンテキスト。
/// roster 不要（freeNote のように teamID/playerID が無い fact だけを見る）の場合は
/// `RosterContext::empty(home, away)` を渡してよい。
///
/// Swift の `[PlayerID: TeamID]` / `Set<PlayerID>?` は決定性のため BTreeMap / BTreeSet で移植
/// （ADR 0001）。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RosterContext {
    pub home_team_id: TeamId,
    pub away_team_id: TeamId,
    /// playerID -> teamID のルックアップ。所属が分かる選手だけ載せれば良い。
    pub player_team_lookup: BTreeMap<PlayerId, TeamId>,
    /// home/away ロスターに実在する player ID の全集合。
    /// `None` なら dangling 検出を行わない（後方互換: roster 不明 / 未登録）。
    /// `Some` の場合、fact が参照する playerID / relatedPlayerID がこの集合に無ければ
    /// `unknownPlayerReference`（削除済み等の無効な参照）として blocking 検出する。
    pub known_player_ids: Option<BTreeSet<PlayerId>>,
}

impl RosterContext {
    pub fn empty(home: TeamId, away: TeamId) -> RosterContext {
        RosterContext {
            home_team_id: home,
            away_team_id: away,
            player_team_lookup: BTreeMap::new(),
            known_player_ids: None,
        }
    }
}

// ── MatchFact dispatch ──

pub fn validate_match_fact(
    fact: &MatchFact,
    configuration: &MatchConfiguration,
    roster: &RosterContext,
) -> Vec<DomainValidationIssue> {
    match &fact.payload {
        MatchFactPayload::Play(play) => validate_play_fact(play, configuration, roster),
        MatchFactPayload::Control(control) => validate_control_fact(control, configuration),
        MatchFactPayload::Possession(possession) => {
            validate_possession_fact(possession, configuration, roster)
        }
    }
}

// ── PossessionFact ──

/// ポゼッション開始の value + context validation（handball-project#154 / #220）。
///
/// 見るのは anchor の値域 / configuration 整合 / end の順序 / team 参照の 4 つだけ。`team_id` は
/// 型で必須なので「欠けている」ケースは validation に来ない。
///
/// **end に置く blocking は `start < end` だけ**（handball-project#220）。end 自体の有無は
/// configuration で分岐しない — `stoppage` が Timer / Video で end の要否を分けているのとは
/// **意図的に非対称**で、ポゼッションの end は「記録方法」ではなく「供給源が終わりを出せたか」で
/// 決まる（CV は goal / ズームを組み合わせられた区間でだけ出せる）。
///
/// **意図的に置いていないルール**（`DOMAIN_VALIDATION_RULES.md`「持たないルール」）:
/// 同一チームの連続禁止 / phase を隙間なく覆う要求 / `.videoHighlight` での禁止 /
/// 「end ≤ 次のポゼッション開始」/「end が phase 範囲内」。severity は一律 blocking なので、
/// これらを足すと供給源の欠測・順序逆転 1 件で試合まるごと import 拒否になる。
pub fn validate_possession_fact(
    fact: &PossessionFact,
    configuration: &MatchConfiguration,
    roster: &RosterContext,
) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    issues.extend(validate_anchor_value(fact.anchor));
    issues.extend(validate_anchor_kind(fact.anchor.kind(), configuration));

    if let Some(end_anchor) = fact.end_anchor {
        issues.extend(validate_anchor_value(end_anchor));
        issues.extend(validate_anchor_kind(end_anchor.kind(), configuration));

        if let (Some(start_seconds), Some(end_seconds)) = (
            progressing_ordering_seconds(fact.anchor),
            progressing_ordering_seconds(end_anchor),
        ) && end_seconds <= start_seconds
        {
            issues.push(DomainValidationIssue::Fact(
                FactValidationError::PossessionEndBeforeStart,
            ));
        }
    }

    if fact.team_id != roster.home_team_id && fact.team_id != roster.away_team_id {
        issues.push(DomainValidationIssue::Fact(
            FactValidationError::UnknownTeamReference {
                team_id: fact.team_id,
            },
        ));
    }

    issues
}

// ── PlayFact ──

pub fn validate_play_fact(
    fact: &PlayFact,
    configuration: &MatchConfiguration,
    roster: &RosterContext,
) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    issues.extend(validate_anchor_value(fact.anchor));
    issues.extend(validate_anchor_kind(fact.anchor.kind(), configuration));

    if let Some(title) = &fact.title
        && title.trim().is_empty()
    {
        issues.push(DomainValidationIssue::Fact(FactValidationError::EmptyTitle));
    }
    if let Some(note) = &fact.note
        && note.trim().is_empty()
    {
        issues.push(DomainValidationIssue::Fact(FactValidationError::EmptyNote));
    }

    if let (Some(p), Some(r)) = (fact.player_id, fact.related_player_id)
        && p == r
    {
        issues.push(DomainValidationIssue::Fact(
            FactValidationError::DuplicatePrimaryAndRelatedPlayer,
        ));
    }

    issues.extend(validate_play_kind_requirements(fact));
    issues.extend(validate_references(fact, roster));

    issues
}

// ── ControlFact ──

pub fn validate_control_fact(
    fact: &ControlFact,
    configuration: &MatchConfiguration,
) -> Vec<DomainValidationIssue> {
    match fact {
        ControlFact::PhaseStart(payload) => validate_phase_start(payload, configuration),
        ControlFact::Stoppage(payload) => validate_stoppage(payload, configuration),
    }
}

fn validate_phase_start(
    payload: &PhaseStartPayload,
    configuration: &MatchConfiguration,
) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    issues.extend(validate_anchor_value(payload.start_anchor));
    issues.extend(validate_anchor_value(payload.end_anchor));
    issues.extend(validate_anchor_kind(
        payload.start_anchor.kind(),
        configuration,
    ));
    issues.extend(validate_anchor_kind(
        payload.end_anchor.kind(),
        configuration,
    ));

    if payload.start_anchor.kind() != payload.end_anchor.kind() {
        issues.push(DomainValidationIssue::Fact(
            FactValidationError::PhaseStartAnchorMismatch,
        ));
    }

    if let (Some(start_seconds), Some(end_seconds)) = (
        primary_elapsed_seconds(payload.start_anchor),
        primary_elapsed_seconds(payload.end_anchor),
    ) && end_seconds <= start_seconds
    {
        issues.push(DomainValidationIssue::Fact(
            FactValidationError::PhaseStartEndBeforeStart,
        ));
    }

    issues
}

fn validate_stoppage(
    payload: &StoppagePayload,
    configuration: &MatchConfiguration,
) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    issues.extend(validate_anchor_value(payload.start_anchor));
    issues.extend(validate_anchor_kind(
        payload.start_anchor.kind(),
        configuration,
    ));

    if let Some(end_anchor) = payload.end_anchor {
        issues.extend(validate_anchor_value(end_anchor));
        issues.extend(validate_anchor_kind(end_anchor.kind(), configuration));

        // Stoppage 中は matchClock が凍結する（start.match == end.match が正常）。
        // よって順序判定は進行する video clock を優先する（video があれば video、無ければ match）。
        if let (Some(start_seconds), Some(end_seconds)) = (
            progressing_ordering_seconds(payload.start_anchor),
            progressing_ordering_seconds(end_anchor),
        ) && end_seconds <= start_seconds
        {
            issues.push(DomainValidationIssue::Fact(
                FactValidationError::StoppageEndBeforeStart,
            ));
        }
    }

    // Capture method × Stoppage endAnchor の整合
    match configuration {
        MatchConfiguration::Timer { .. } => {
            if payload.end_anchor.is_some() {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::StoppageEndPresentInTimerMode { kind: payload.kind },
                ));
            }
        }
        MatchConfiguration::Video(_) => {
            if payload.end_anchor.is_none() {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::StoppageEndNilInVideoMode { kind: payload.kind },
                ));
            }
        }
        MatchConfiguration::VideoHighlight(_) => {
            // Stoppage 自体が R9 で禁止される（fact_log_validator で検出）
        }
    }

    // kind × note の整合
    match payload.kind {
        StoppageKind::Timeout => {
            if payload.note.is_some() {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::TimeoutHasNote,
                ));
            }
        }
        StoppageKind::Pause => {
            if let Some(note) = &payload.note
                && note.trim().is_empty()
            {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::EmptyStoppageNote,
                ));
            }
        }
    }

    issues
}

// ── Anchor 値の範囲 ──

fn validate_anchor_value(anchor: FactAnchor) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();
    match anchor {
        FactAnchor::MatchClock(mc) => {
            issues.extend(match_clock_value_issue(mc.elapsed_seconds));
        }
        FactAnchor::VideoClock(vc) => {
            issues.extend(video_clock_value_issue(vc.elapsed_seconds));
        }
        FactAnchor::Both {
            match_clock: mc,
            video_clock: vc,
        } => {
            issues.extend(match_clock_value_issue(mc.elapsed_seconds));
            issues.extend(video_clock_value_issue(vc.elapsed_seconds));
        }
    }
    issues
}

/// 非有限（NaN / ±∞）を負値より先に見る。`NaN < 0.0` は false になるため、
/// 順序を逆にすると非有限が素通りする（handball-project#91）。
fn match_clock_value_issue(seconds: f64) -> Option<DomainValidationIssue> {
    if !seconds.is_finite() {
        Some(DomainValidationIssue::Fact(
            FactValidationError::NonFiniteMatchClock,
        ))
    } else if seconds < 0.0 {
        Some(DomainValidationIssue::Fact(
            FactValidationError::NegativeMatchClock,
        ))
    } else {
        None
    }
}

/// [`match_clock_value_issue`] の video clock 版。
fn video_clock_value_issue(seconds: f64) -> Option<DomainValidationIssue> {
    if !seconds.is_finite() {
        Some(DomainValidationIssue::Fact(
            FactValidationError::NonFiniteVideoClock,
        ))
    } else if seconds < 0.0 {
        Some(DomainValidationIssue::Fact(
            FactValidationError::NegativeVideoClock,
        ))
    } else {
        None
    }
}

// ── Anchor kind の configuration 整合 ──

fn validate_anchor_kind(
    actual: FactAnchorKind,
    configuration: &MatchConfiguration,
) -> Vec<DomainValidationIssue> {
    let allowed = allowed_anchor_kinds(configuration);
    if allowed.contains(&actual) {
        return Vec::new();
    }
    vec![DomainValidationIssue::Fact(
        FactValidationError::InvalidAnchorForConfiguration {
            configuration: configuration.kind(),
            actual,
            allowed,
        },
    )]
}

fn allowed_anchor_kinds(config: &MatchConfiguration) -> BTreeSet<FactAnchorKind> {
    match config {
        MatchConfiguration::Timer { .. } => BTreeSet::from([FactAnchorKind::MatchClock]),
        MatchConfiguration::Video(_) | MatchConfiguration::VideoHighlight(_) => {
            BTreeSet::from([FactAnchorKind::VideoClock, FactAnchorKind::Both])
        }
    }
}

/// fact が configuration に許されない anchor を 1 つでも持つか
/// （移行が途中で止まった試合の判定材料 — handball-project#351）。
///
/// **`validate_match_fact` の anchor 整合と同じ材料 `allowed_anchor_kinds` を共有する。**
/// 「どの anchor がどの configuration で許されるか」をこのファイルの外へ書き写さないために
/// 公開する（呼び出し側が kind を並べ直すと、許可集合が 2 箇所に分かれる）。
///
/// roster を取らないので参照整合は見ない。anchor と configuration の整合だけを見る。
pub fn has_anchor_mismatched_with_configuration(
    fact: &MatchFact,
    configuration: &MatchConfiguration,
) -> bool {
    let allowed = allowed_anchor_kinds(configuration);
    anchor_kinds(fact)
        .into_iter()
        .any(|kind| !allowed.contains(&kind))
}

/// fact が持つ anchor の kind を**全部**（range を持つものは start / end とも）返す。
fn anchor_kinds(fact: &MatchFact) -> Vec<FactAnchorKind> {
    match &fact.payload {
        MatchFactPayload::Play(play) => vec![play.anchor.kind()],
        MatchFactPayload::Possession(possession) => {
            let mut kinds = vec![possession.anchor.kind()];
            if let Some(end_anchor) = possession.end_anchor {
                kinds.push(end_anchor.kind());
            }
            kinds
        }
        MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => {
            vec![payload.start_anchor.kind(), payload.end_anchor.kind()]
        }
        MatchFactPayload::Control(ControlFact::Stoppage(payload)) => {
            let mut kinds = vec![payload.start_anchor.kind()];
            if let Some(end_anchor) = payload.end_anchor {
                kinds.push(end_anchor.kind());
            }
            kinds
        }
    }
}

// ── PlayFact kind ごとの必須項目 ──

fn validate_play_kind_requirements(fact: &PlayFact) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    match fact.kind {
        PlayEventKind::Goal
        | PlayEventKind::ShotMissed
        | PlayEventKind::YellowCard
        | PlayEventKind::TwoMinuteSuspension
        | PlayEventKind::RedCard => {
            if fact.player_id.is_none() {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::MissingPlayerForPlayKind { kind: fact.kind },
                ));
            }
        }
        PlayEventKind::FreeNote => {
            // teamID / playerID / note / title すべて optional
            // （anchor だけの「マーカー freeNote」も valid）。
            // freeNoteHasNoContent は将来仕様変更に備えて enum に残すが、現仕様では発火しない。
        }
    }

    issues
}

// ── team / player 参照整合 ──

fn validate_references(fact: &PlayFact, roster: &RosterContext) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    if let Some(team_id) = fact.team_id
        && team_id != roster.home_team_id
        && team_id != roster.away_team_id
    {
        issues.push(DomainValidationIssue::Fact(
            FactValidationError::UnknownTeamReference { team_id },
        ));
    }

    if let Some(player_id) = fact.player_id {
        if let Some(known) = &roster.known_player_ids
            && !known.contains(&player_id)
        {
            // roster に実在しない（dangling / 別チーム）参照は blocking。
            issues.push(DomainValidationIssue::Fact(
                FactValidationError::UnknownPlayerReference { player_id },
            ));
        } else if let Some(&known_team) = roster.player_team_lookup.get(&player_id) {
            if known_team != roster.home_team_id && known_team != roster.away_team_id {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::UnknownPlayerReference { player_id },
                ));
            } else if let Some(team_id) = fact.team_id
                && known_team != team_id
            {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::PlayerTeamMismatch { player_id, team_id },
                ));
            }
        }
    }

    if let Some(related_id) = fact.related_player_id {
        if let Some(known) = &roster.known_player_ids
            && !known.contains(&related_id)
        {
            issues.push(DomainValidationIssue::Fact(
                FactValidationError::UnknownPlayerReference {
                    player_id: related_id,
                },
            ));
        } else if let Some(&known_team) = roster.player_team_lookup.get(&related_id) {
            if known_team != roster.home_team_id && known_team != roster.away_team_id {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::UnknownPlayerReference {
                        player_id: related_id,
                    },
                ));
            } else if let Some(team_id) = fact.team_id
                && known_team != team_id
            {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::RelatedPlayerTeamMismatch {
                        player_id: related_id,
                        team_id,
                    },
                ));
            }
        }
    }

    issues
}

// ── Helpers ──

/// anchor の primary 累積秒（matchClock 優先、なければ videoClock）。
fn primary_elapsed_seconds(anchor: FactAnchor) -> Option<f64> {
    anchor
        .match_elapsed_seconds()
        .or(anchor.video_elapsed_seconds())
}

/// 「必ず進む時計」での順序判定用秒（videoClock 優先、なければ matchClock）。
///
/// matchClock は Stoppage 中に凍結するので、start / end が停止をまたぐ・停止に接する fact では
/// `Both` anchor が同じ秒を持ちうる。videoClock は止まらないため、`start < end` を偽陽性なしで
/// 判定できるのはこちら。Stoppage（区間が停止そのもの）と Possession（区間がタイムアウトを
/// またぎうる）の両方が使う。
///
/// `PhaseStart` は `primary_elapsed_seconds`（matchClock 優先）のまま — 規定長を
/// `end - start` で導出する定義が matchClock 側にあるため。
fn progressing_ordering_seconds(anchor: FactAnchor) -> Option<f64> {
    anchor
        .video_elapsed_seconds()
        .or(anchor.match_elapsed_seconds())
}
