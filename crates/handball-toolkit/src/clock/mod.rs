//! 時計と anchor。移植元: `Clock/` ディレクトリ（ADR 0001 ミラー表）。

mod fact_anchor;
mod match_clock;
mod video_clock;

pub use fact_anchor::{FactAnchor, FactAnchorKind};
pub use match_clock::MatchClock;
pub use video_clock::VideoClock;
