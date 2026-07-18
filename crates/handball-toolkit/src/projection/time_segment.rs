//! 移植元: `Projection/TimeSegment.swift`。

use serde::{Deserialize, Serialize};

use crate::configuration::PhaseKind;
use crate::facts::StoppageKind;
use crate::ids::FactId;

/// `TimeSegment.Kind`（Swift の nested enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum TimeSegmentKind {
    /// タイマー動作中（matchTime と videoTime が 1:1 で進む）。shootout phase の running は degenerate（matchClock 固定）。
    Running,
    /// タイマー停止中（videoTime は進むが matchTime は固定）。stoppage_kind で timeout / pause を区別。
    Stopped,
}

/// fact log から構築される時間区間。
///
/// running / stopped の 2 種で、video↔match の対応を保持する。
/// 旧設計では phase でグルーピングしていたが、新設計では累積秒ベースの flat list に変更。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct TimeSegment {
    pub kind: TimeSegmentKind,
    /// この segment が属する phase の kind。phase 外（= 試合前 / phase 間）なら None。
    pub phase_kind: Option<PhaseKind>,
    pub match_elapsed_start: f64,
    /// 開区間（進行中）なら None。
    pub match_elapsed_end: Option<f64>,
    pub video_elapsed_start: Option<f64>,
    pub video_elapsed_end: Option<f64>,
    /// この segment を開始した fact の ID。running なら PhaseStart fact / 前 Stoppage の終了、stopped なら Stoppage fact。
    pub start_fact_id: Option<FactId>,
    pub end_fact_id: Option<FactId>,
    /// stopped segment のみ。timeout / pause の判別に使う。
    pub stoppage_kind: Option<StoppageKind>,
}

impl TimeSegment {
    pub fn match_elapsed_duration(&self) -> Option<f64> {
        let match_elapsed_end = self.match_elapsed_end?;
        match self.kind {
            TimeSegmentKind::Running => Some(match_elapsed_end - self.match_elapsed_start),
            TimeSegmentKind::Stopped => Some(0.0),
        }
    }

    pub fn video_elapsed_duration(&self) -> Option<f64> {
        let video_elapsed_start = self.video_elapsed_start?;
        let video_elapsed_end = self.video_elapsed_end?;
        Some(video_elapsed_end - video_elapsed_start)
    }

    pub fn contains_video_elapsed(&self, video: f64) -> bool {
        let Some(start) = self.video_elapsed_start else {
            return false;
        };
        if let Some(end) = self.video_elapsed_end {
            return video >= start && video < end;
        }
        video >= start
    }

    pub fn contains_match_elapsed(&self, match_elapsed: f64) -> bool {
        if let Some(end) = self.match_elapsed_end {
            return match_elapsed >= self.match_elapsed_start && match_elapsed < end;
        }
        match_elapsed >= self.match_elapsed_start
    }

    /// segment 内の videoElapsed → matchElapsed 変換。
    pub fn match_elapsed_for_video_elapsed(&self, video: f64) -> f64 {
        match self.kind {
            TimeSegmentKind::Running => {
                // shootout の degenerate running segment: matchClock 累積秒は phase 開始値で固定。
                if self.phase_kind == Some(PhaseKind::Shootout) {
                    return self.match_elapsed_start;
                }
                let Some(video_start) = self.video_elapsed_start else {
                    return self.match_elapsed_start;
                };
                self.match_elapsed_start + (video - video_start)
            }
            TimeSegmentKind::Stopped => self.match_elapsed_start,
        }
    }

    /// segment 内の matchElapsed → videoElapsed 変換（videoStart 未知なら None）。
    pub fn video_elapsed_for_match_elapsed(&self, match_elapsed: f64) -> Option<f64> {
        let video_start = self.video_elapsed_start?;
        match self.kind {
            TimeSegmentKind::Running => {
                // shootout: matchClock 累積秒は phase 全体で固定 = 起点しか戻せない。
                if self.phase_kind == Some(PhaseKind::Shootout) {
                    return Some(video_start);
                }
                Some(video_start + (match_elapsed - self.match_elapsed_start))
            }
            TimeSegmentKind::Stopped => Some(video_start),
        }
    }
}
