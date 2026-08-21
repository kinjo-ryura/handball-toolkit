//! projection（fact 列からの導出）。移植元: `Projection/` ディレクトリ。
//!
//! ADR 0001 ミラー表の通り、Swift ファイルごとに公開サブモジュールを持つ
//! （`projection::time_segment` ↔ `Projection/TimeSegment.swift` など）。
//! 主要型はこのモジュール直下にも re-export する。

pub mod live_match;
pub mod possession;
pub mod score_progression;
pub mod segment_resolver;
pub mod summary;
pub mod time_segment;
pub mod timeline;

pub use live_match::{AvailableActions, LiveMatchProjection, MatchTimerState};
pub use possession::{PossessionProjection, PossessionSegment};
pub use score_progression::{
    ScoreProgressionPhaseSpan, ScoreProgressionPoint, ScoreProgressionProjection,
};
pub use segment_resolver::{Phase, SegmentResolver};
pub use summary::{PhaseSummaryLine, PlayerStatLine, SummaryProjection, TeamSummaryLine};
pub use time_segment::{TimeSegment, TimeSegmentKind};
pub use timeline::{ResolvedFact, TimelineProjection};
