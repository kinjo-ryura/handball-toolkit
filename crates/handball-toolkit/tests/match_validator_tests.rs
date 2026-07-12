//! 移植元: `Tests/RecorderDomainTests/MatchValidatorTests.swift`。

use std::collections::BTreeSet;

use chrono::DateTime;
use handball_toolkit::configuration::MatchConfiguration;
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::ids::TeamId;
use handball_toolkit::validation::{DomainValidationIssue, MatchValidationError};
use handball_toolkit::validators::validate_match;
use uuid::Uuid;

/// Swift suite の `makeMatch(title:homeTeamID:awayTeamID:rosterSelection:)` 相当。
fn make_match(
    title: Option<&str>,
    home_team_id: TeamId,
    away_team_id: TeamId,
    roster_selection: RosterSelection,
) -> Match {
    Match {
        id: Uuid::new_v4(),
        title: title.map(str::to_owned),
        date: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        home_team_id,
        away_team_id,
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        roster_selection,
        is_home_on_left: true,
    }
}

fn make_default_match() -> Match {
    make_match(
        Some("Test"),
        Uuid::new_v4(),
        Uuid::new_v4(),
        RosterSelection::default(),
    )
}

#[test]
fn clean_match_has_no_issues() {
    let match_ = make_default_match();
    assert!(validate_match(&match_).is_empty());
}

#[test]
fn same_team_on_both_sides_is_blocking() {
    let team_id = Uuid::new_v4();
    let match_ = make_match(Some("Test"), team_id, team_id, RosterSelection::default());
    let issues = validate_match(&match_);
    assert_eq!(
        issues,
        vec![DomainValidationIssue::Match(
            MatchValidationError::SameTeamOnBothSides
        )]
    );
}

#[test]
fn none_title_is_allowed() {
    let match_ = make_match(
        None,
        Uuid::new_v4(),
        Uuid::new_v4(),
        RosterSelection::default(),
    );
    assert!(validate_match(&match_).is_empty());
}

#[test]
fn empty_title_string_is_blocking() {
    let match_ = make_match(
        Some("   "),
        Uuid::new_v4(),
        Uuid::new_v4(),
        RosterSelection::default(),
    );
    let issues = validate_match(&match_);
    assert_eq!(
        issues,
        vec![DomainValidationIssue::Match(
            MatchValidationError::EmptyTitle
        )]
    );
}

#[test]
fn overlapping_roster_is_blocking() {
    let player = Uuid::new_v4();
    let roster = RosterSelection {
        benched_player_ids: BTreeSet::from([player]),
        out_of_roster_player_ids: BTreeSet::from([player, Uuid::new_v4()]),
    };
    let match_ = make_match(Some("Test"), Uuid::new_v4(), Uuid::new_v4(), roster);
    let issues = validate_match(&match_);
    assert_eq!(
        issues,
        vec![DomainValidationIssue::Match(
            MatchValidationError::OverlappingRosterSelections {
                player_ids: BTreeSet::from([player]),
            }
        )]
    );
}
