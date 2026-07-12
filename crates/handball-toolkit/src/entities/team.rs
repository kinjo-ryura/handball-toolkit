//! 移植元: `Entities/Team.swift`。

use serde::{Deserialize, Serialize};

use crate::ids::TeamId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: TeamId,
    pub name: String,
}
