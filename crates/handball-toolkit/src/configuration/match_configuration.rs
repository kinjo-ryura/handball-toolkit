//! 移植元: `Configuration/MatchConfiguration.swift`。

use serde::{Deserialize, Serialize};

use super::VideoSource;

/// 試合の有効パターンを variant で網羅的に表現する sum type。
///
/// 旧 `RecordingMode` / `ContentKind` / `CaptureMethod` 三組み合わせを置き換え。
/// 型レベルで illegal state（`Timer + videoSource あり` / `Video + videoSource なし` 等）を排除。
///
/// Swift 版の `contentKind` / `ContentKind` は移植しない（ADR 0001。ドメイン内部で未使用の
/// UI helper のため。必要なシェルは variant の match で自前導出する）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MatchConfiguration {
    /// タイマーモード（動画なし、フル試合）。`phase_duration_seconds` は phase 開始時の
    /// デフォルト endAnchor を埋めるための値であり、タイマー UI 上限値・終了アラート判定に使う。
    Timer { phase_duration_seconds: f64 },
    /// 動画モード（動画あり、フル試合）。時計の source of truth は videoClock。
    Video(VideoSource),
    /// 動画ハイライトモード（動画あり、ハイライト集）。phase 構造なし、Stoppage 概念なし。
    VideoHighlight(VideoSource),
}

/// `invalidAnchorForConfiguration` 等で「どの variant 由来か」を伝えるための raw kind。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchConfigurationKind {
    Timer,
    Video,
    VideoHighlight,
}

impl MatchConfigurationKind {
    /// Swift `CaseIterable.allCases` 相当。
    pub const ALL_CASES: [MatchConfigurationKind; 3] = [
        MatchConfigurationKind::Timer,
        MatchConfigurationKind::Video,
        MatchConfigurationKind::VideoHighlight,
    ];
}

impl MatchConfiguration {
    pub fn kind(&self) -> MatchConfigurationKind {
        match self {
            MatchConfiguration::Timer { .. } => MatchConfigurationKind::Timer,
            MatchConfiguration::Video(_) => MatchConfigurationKind::Video,
            MatchConfiguration::VideoHighlight(_) => MatchConfigurationKind::VideoHighlight,
        }
    }

    /// 試合の時計 source of truth（UI helper、source of truth は常に `MatchConfiguration` 自身の variant）。
    pub fn capture_method(&self) -> CaptureMethod {
        match self {
            MatchConfiguration::Timer { .. } => CaptureMethod::ManualClock,
            MatchConfiguration::Video(_) | MatchConfiguration::VideoHighlight(_) => {
                CaptureMethod::Video
            }
        }
    }

    /// 動画 source（UI helper、`Timer` のときは None）。
    pub fn video_source(&self) -> Option<&VideoSource> {
        match self {
            MatchConfiguration::Timer { .. } => None,
            MatchConfiguration::Video(source) | MatchConfiguration::VideoHighlight(source) => {
                Some(source)
            }
        }
    }

    /// `Timer` の phase_duration_seconds（他 variant では None）。
    pub fn phase_duration_seconds(&self) -> Option<f64> {
        match self {
            MatchConfiguration::Timer {
                phase_duration_seconds,
            } => Some(*phase_duration_seconds),
            MatchConfiguration::Video(_) | MatchConfiguration::VideoHighlight(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureMethod {
    ManualClock,
    Video,
}
