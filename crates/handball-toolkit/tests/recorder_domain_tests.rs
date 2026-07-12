//! 移植元: `Tests/RecorderDomainTests/RecorderDomainTests.swift`。
//!
//! Swift の `moduleExposesCoreTypes` は `TimeSegment`（P4 `projection::time_segment`）を
//! 参照するため、そのモジュールの移植時に合わせて移植する。

use handball_toolkit::clock::{FactAnchor, FactAnchorKind, MatchClock, VideoClock};

#[test]
fn fact_anchor_accessors_expose_contained_clocks() {
    let match_clock = MatchClock {
        elapsed_seconds: 120.0,
    };
    let video = VideoClock {
        elapsed_seconds: 754.0,
    };

    let only = FactAnchor::MatchClock(match_clock);
    assert_eq!(only.match_clock(), Some(match_clock));
    assert_eq!(only.video_clock(), None);
    assert_eq!(only.kind(), FactAnchorKind::MatchClock);

    let video_only = FactAnchor::VideoClock(video);
    assert_eq!(video_only.match_clock(), None);
    assert_eq!(video_only.video_clock(), Some(video));
    assert_eq!(video_only.kind(), FactAnchorKind::VideoClock);

    let both = FactAnchor::Both {
        match_clock,
        video_clock: video,
    };
    assert_eq!(both.match_clock(), Some(match_clock));
    assert_eq!(both.video_clock(), Some(video));
    assert_eq!(both.kind(), FactAnchorKind::Both);
}
