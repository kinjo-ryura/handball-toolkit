//! 移植元: `Clock/FactAnchor.swift`。

use serde::{Deserialize, Serialize};

use super::{MatchClock, VideoClock};

/// その事実を何基準で観測したか。
///
/// - `MatchClock`: 手動時計で記録（タイマーモード）
/// - `VideoClock`: 動画を見ながら記録（動画モード / ハイライト）
/// - `Both`: 強制 sync point（動画カット復旧などの override 専用、平常時は使わない）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FactAnchor {
    MatchClock(MatchClock),
    VideoClock(VideoClock),
    Both {
        match_clock: MatchClock,
        video_clock: VideoClock,
    },
}

/// `invalidAnchorForConfiguration` 等で actual / allowed の表現に使う raw kind。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FactAnchorKind {
    MatchClock,
    VideoClock,
    Both,
}

impl FactAnchorKind {
    /// Swift `CaseIterable.allCases` 相当。
    pub const ALL_CASES: [FactAnchorKind; 3] = [
        FactAnchorKind::MatchClock,
        FactAnchorKind::VideoClock,
        FactAnchorKind::Both,
    ];
}

impl FactAnchor {
    pub fn kind(&self) -> FactAnchorKind {
        match self {
            FactAnchor::MatchClock(_) => FactAnchorKind::MatchClock,
            FactAnchor::VideoClock(_) => FactAnchorKind::VideoClock,
            FactAnchor::Both { .. } => FactAnchorKind::Both,
        }
    }

    pub fn match_clock(&self) -> Option<MatchClock> {
        match self {
            FactAnchor::MatchClock(clock)
            | FactAnchor::Both {
                match_clock: clock, ..
            } => Some(*clock),
            FactAnchor::VideoClock(_) => None,
        }
    }

    pub fn video_clock(&self) -> Option<VideoClock> {
        match self {
            FactAnchor::VideoClock(clock)
            | FactAnchor::Both {
                video_clock: clock, ..
            } => Some(*clock),
            FactAnchor::MatchClock(_) => None,
        }
    }

    /// matchClock があれば matchClock.elapsedSeconds、なければ None。
    pub fn match_elapsed_seconds(&self) -> Option<f64> {
        self.match_clock().map(|clock| clock.elapsed_seconds)
    }

    /// videoClock があれば videoClock.elapsedSeconds、なければ None。
    pub fn video_elapsed_seconds(&self) -> Option<f64> {
        self.video_clock().map(|clock| clock.elapsed_seconds)
    }
}
