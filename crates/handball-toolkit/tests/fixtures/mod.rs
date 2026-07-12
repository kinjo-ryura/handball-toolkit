//! テストフィクスチャ集約（PORTING.md「テスト移植のメモ」）。
//! 移植元: 各 Swift テスト suite の private helper 群。
//!
//! Swift 版は `recordedAt: .init(timeIntervalSince1970: 0)`、fact ID は `FactID()`（ランダム）。

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, StoppageKind, StoppagePayload,
};
use handball_toolkit::ids::FactId;
use uuid::Uuid;

pub fn epoch() -> DateTime<Utc> {
    DateTime::from_timestamp(0, 0).unwrap()
}

/// SegmentResolverAdvancedTests の `videoOnlyPhase`。
pub fn video_only_phase(start: f64, end: f64) -> MatchFact {
    video_phase(Uuid::new_v4(), start, end)
}

/// SegmentResolverAdvancedTests の `videoPhase`。
pub fn video_phase(id: FactId, start: f64, end: f64) -> MatchFact {
    MatchFact {
        id,
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: start,
            }),
            end_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end,
            }),
        })),
    }
}

/// SegmentResolverAdvancedTests の `videoStoppage`。
pub fn video_stoppage(id: FactId, kind: StoppageKind, start: f64, end: f64) -> MatchFact {
    MatchFact {
        id,
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: start,
            }),
            end_anchor: Some(FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end,
            })),
            note: None,
        })),
    }
}

/// SegmentResolverAdvancedTests の `phaseStartBoth`。
pub fn phase_start_both(
    kind: PhaseKind,
    match_start: f64,
    video_start: f64,
    match_end: f64,
    video_end: f64,
) -> MatchFact {
    MatchFact {
        id: Uuid::new_v4(),
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind,
            start_anchor: FactAnchor::Both {
                match_clock: MatchClock {
                    elapsed_seconds: match_start,
                },
                video_clock: VideoClock {
                    elapsed_seconds: video_start,
                },
            },
            end_anchor: FactAnchor::Both {
                match_clock: MatchClock {
                    elapsed_seconds: match_end,
                },
                video_clock: VideoClock {
                    elapsed_seconds: video_end,
                },
            },
        })),
    }
}

/// SegmentResolverAdvancedTests の `shootoutPhase`。
pub fn shootout_phase(start: f64, end: f64) -> MatchFact {
    MatchFact {
        id: Uuid::new_v4(),
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Shootout,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: start,
            }),
            end_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end,
            }),
        })),
    }
}

/// タイマーモードの PhaseStart（matchClock anchor）。
pub fn timer_phase(id: FactId, start: f64, end: f64) -> MatchFact {
    MatchFact {
        id,
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: start,
            }),
            end_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: end,
            }),
        })),
    }
}

/// タイマーモードの Stoppage marker（endAnchor なし）。
pub fn timer_stoppage_marker(id: FactId, kind: StoppageKind, start: f64) -> MatchFact {
    MatchFact {
        id,
        recorded_at: epoch(),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: start,
            }),
            end_anchor: None,
            note: None,
        })),
    }
}
