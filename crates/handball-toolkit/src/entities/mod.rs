//! エンティティ。移植元: `Entities/` ディレクトリ（ADR 0001 ミラー表）。

mod r#match;
mod player;
mod team;

pub use r#match::{Match, RosterSelection};
pub use player::{Player, PlayerPhoto};
pub use team::Team;
