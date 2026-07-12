//! 試合設定。移植元: `Configuration/` ディレクトリ（ADR 0001 ミラー表）。

mod match_configuration;
mod phase_kind;
mod video_source;

pub use match_configuration::{CaptureMethod, MatchConfiguration, MatchConfigurationKind};
pub use phase_kind::PhaseKind;
pub use video_source::{VideoProvider, VideoSource};
