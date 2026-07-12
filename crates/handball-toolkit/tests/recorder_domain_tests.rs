//! 移植元: `Tests/RecorderDomainTests/RecorderDomainTests.swift`。
//!
//! Swift の `moduleExposesCoreTypes` は `TimeSegment`（P4 `projection::time_segment`）を
//! 参照するため、そのモジュールの移植時に合わせて移植する。

use handball_toolkit::clock::{FactAnchor, FactAnchorKind, MatchClock, VideoClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};

#[test]
fn match_configuration_round_trips_via_serde() {
    let cases: [MatchConfiguration; 3] = [
        MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        MatchConfiguration::Video(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "abc".to_owned(),
        }),
        MatchConfiguration::VideoHighlight(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "def".to_owned(),
        }),
    ];
    for original in &cases {
        let data = serde_json::to_string(original).unwrap();
        let decoded: MatchConfiguration = serde_json::from_str(&data).unwrap();
        assert_eq!(&decoded, original);
    }
}

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

#[test]
fn phase_kind_has_only_two_cases() {
    assert_eq!(
        PhaseKind::ALL_CASES,
        [PhaseKind::Regular, PhaseKind::Shootout]
    );
}
