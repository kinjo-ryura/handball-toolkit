//! 移植元: `Tests/RecorderDomainTests/RosterReferenceValidationTests.swift`。
//!
//! RosterContext.known_player_ids による dangling（削除済み等）player 参照の blocking 検出。
//! HandballRecorder CONTEXT.md「dangling は blocking 検出」要件に対応する回帰テスト。

use std::collections::{BTreeMap, BTreeSet};

use handball_toolkit::clock::{FactAnchor, MatchClock};
use handball_toolkit::configuration::MatchConfiguration;
use handball_toolkit::facts::{PlayEventKind, PlayFact};
use handball_toolkit::ids::{PlayerId, TeamId};
use handball_toolkit::validation::{DomainValidationIssue, FactValidationError};
use handball_toolkit::validators::{RosterContext, validate_play_fact};
use uuid::Uuid;

struct Ctx {
    home: TeamId,
    away: TeamId,
    rostered: PlayerId,
}

fn ctx() -> Ctx {
    Ctx {
        home: Uuid::new_v4(),
        away: Uuid::new_v4(),
        rostered: Uuid::new_v4(),
    }
}

fn roster(c: &Ctx) -> RosterContext {
    RosterContext {
        home_team_id: c.home,
        away_team_id: c.away,
        player_team_lookup: BTreeMap::from([(c.rostered, c.home)]),
        known_player_ids: Some(BTreeSet::from([c.rostered])),
    }
}

fn play(kind: PlayEventKind, player_id: Option<PlayerId>, related: Option<PlayerId>) -> PlayFact {
    PlayFact {
        kind,
        team_id: None,
        player_id,
        related_player_id: related,
        anchor: FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 0.0,
        }),
        title: None,
        note: None,
    }
}

fn validate(fact: &PlayFact, roster: &RosterContext) -> Vec<DomainValidationIssue> {
    validate_play_fact(
        fact,
        &MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        roster,
    )
}

fn has_unknown_player(issues: &[DomainValidationIssue]) -> bool {
    issues.iter().any(|issue| {
        matches!(
            issue,
            DomainValidationIssue::Fact(FactValidationError::UnknownPlayerReference { .. })
        )
    })
}

// ── dangling 検出 ──

#[test]
fn dangling_player_reference_is_flagged() {
    let c = ctx();
    let dangling = Uuid::new_v4();
    let fact = play(PlayEventKind::Goal, Some(dangling), None);
    let issues = validate(&fact, &roster(&c));
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::UnknownPlayerReference {
            player_id: dangling
        }
    )));
}

#[test]
fn dangling_related_player_reference_is_flagged() {
    let c = ctx();
    let dangling_related = Uuid::new_v4();
    let fact = play(
        PlayEventKind::TwoMinuteSuspension,
        Some(c.rostered),
        Some(dangling_related),
    );
    let issues = validate(&fact, &roster(&c));
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::UnknownPlayerReference {
            player_id: dangling_related
        }
    )));
}

// ── 正常系 / 後方互換 ──

#[test]
fn rostered_player_reference_is_valid() {
    let c = ctx();
    let fact = play(PlayEventKind::Goal, Some(c.rostered), None);
    let issues = validate(&fact, &roster(&c));
    assert!(!has_unknown_player(&issues));
}

/// known_player_ids が None (= empty roster) なら dangling 検出を行わない (後方互換)。
#[test]
fn none_known_player_ids_skips_dangling_detection() {
    let c = ctx();
    let roster = RosterContext::empty(c.home, c.away);
    let fact = play(PlayEventKind::Goal, Some(Uuid::new_v4()), None);
    let issues = validate(&fact, &roster);
    assert!(!has_unknown_player(&issues));
}
