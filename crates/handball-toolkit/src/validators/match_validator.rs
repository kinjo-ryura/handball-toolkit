//! 移植元: `Validators/MatchValidator.swift`。

use std::collections::BTreeSet;

use crate::entities::Match;
use crate::ids::PlayerId;
use crate::validation::{DomainValidationIssue, MatchValidationError};

pub fn validate_match(match_: &Match) -> Vec<DomainValidationIssue> {
    let mut issues: Vec<DomainValidationIssue> = Vec::new();

    if match_.home_team_id == match_.away_team_id {
        issues.push(DomainValidationIssue::Match(
            MatchValidationError::SameTeamOnBothSides,
        ));
    }

    if let Some(title) = &match_.title
        && title.trim().is_empty()
    {
        issues.push(DomainValidationIssue::Match(
            MatchValidationError::EmptyTitle,
        ));
    }

    let overlap: BTreeSet<PlayerId> = match_
        .roster_selection
        .benched_player_ids
        .intersection(&match_.roster_selection.out_of_roster_player_ids)
        .copied()
        .collect();
    if !overlap.is_empty() {
        issues.push(DomainValidationIssue::Match(
            MatchValidationError::OverlappingRosterSelections {
                player_ids: overlap,
            },
        ));
    }

    issues
}
