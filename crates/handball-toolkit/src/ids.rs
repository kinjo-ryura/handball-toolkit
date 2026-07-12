//! 識別子の type alias 群。移植元: `Identifiers.swift`。
//!
//! Swift 同様 type alias のまま忠実移植する（ADR 0001。newtype 化による型安全強化は
//! パリティ完走後の別タスク — handball-project#52）。

use uuid::Uuid;

pub type MatchId = Uuid;
pub type TeamId = Uuid;
pub type PlayerId = Uuid;
pub type FactId = Uuid;
