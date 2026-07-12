//! 移植元: `Validation/MatchValidationError.swift`。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::ids::PlayerId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "code",
    content = "params",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MatchValidationError {
    SameTeamOnBothSides,
    EmptyTitle,
    OverlappingRosterSelections { player_ids: BTreeSet<PlayerId> },
}
