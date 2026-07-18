//! 移植元: `Validators/FactValidator.swift`。
//!
//! 1 件の MatchFact（PlayFact / ControlFact）の value + context validation。
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
    StoppageKind, StoppagePayload,
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
    }
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
            stoppage_ordering_seconds(payload.start_anchor),
            stoppage_ordering_seconds(end_anchor),
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
            if mc.elapsed_seconds < 0.0 {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::NegativeMatchClock,
                ));
            }
        }
        FactAnchor::VideoClock(vc) => {
            if vc.elapsed_seconds < 0.0 {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::NegativeVideoClock,
                ));
            }
        }
        FactAnchor::Both {
            match_clock: mc,
            video_clock: vc,
        } => {
            if mc.elapsed_seconds < 0.0 {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::NegativeMatchClock,
                ));
            }
            if vc.elapsed_seconds < 0.0 {
                issues.push(DomainValidationIssue::Fact(
                    FactValidationError::NegativeVideoClock,
                ));
            }
        }
    }
    issues
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

/// Stoppage の順序判定用秒（videoClock 優先、なければ matchClock）。
/// Stoppage 中は matchClock が凍結するため、`Both` では video で start<end を判定する。
fn stoppage_ordering_seconds(anchor: FactAnchor) -> Option<f64> {
    anchor
        .video_elapsed_seconds()
        .or(anchor.match_elapsed_seconds())
}
