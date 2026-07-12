//! projection（fact 列からの導出）。移植元: `Projection/` ディレクトリ。
//!
//! ADR 0001 ミラー表の通り、Swift ファイルごとに公開サブモジュールを持つ
//! （`projection::time_segment` ↔ `Projection/TimeSegment.swift` など）。
//! 主要型はこのモジュール直下にも re-export する。

pub mod segment_resolver;
pub mod time_segment;

pub use segment_resolver::SegmentResolver;
pub use time_segment::{TimeSegment, TimeSegmentKind};
