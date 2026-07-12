//! 移植元: `Tests/RecorderDomainTests/RecorderDomainTests.swift`。
//!
//! Swift の `moduleExposesCoreTypes` は `TimeSegment`（P4 `projection::time_segment`）を
//! 参照するため、そのモジュールの移植時に合わせて移植する。

use chrono::DateTime;
use handball_toolkit::clock::{FactAnchor, FactAnchorKind, MatchClock, VideoClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
use uuid::Uuid;

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
fn match_fact_payload_round_trips_via_serde() {
    let home_id = Uuid::new_v4();
    let player_id = Uuid::new_v4();
    let fact = MatchFact {
        id: Uuid::new_v4(),
        recorded_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        payload: MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: Some(home_id),
            player_id: Some(player_id),
            related_player_id: None,
            anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: 495.0,
            }),
            title: None,
            note: None,
        }),
    };
    let data = serde_json::to_string(&fact).unwrap();
    let decoded: MatchFact = serde_json::from_str(&data).unwrap();
    assert_eq!(decoded, fact);
}

#[test]
fn control_fact_phase_start_round_trip() {
    let fact = ControlFact::PhaseStart(PhaseStartPayload {
        kind: PhaseKind::Regular,
        start_anchor: FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 100.0,
        }),
        end_anchor: FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 1900.0,
        }),
    });
    let data = serde_json::to_string(&fact).unwrap();
    let decoded: ControlFact = serde_json::from_str(&data).unwrap();
    assert_eq!(decoded, fact);
}

#[test]
fn control_fact_stoppage_round_trip() {
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Timeout,
        start_anchor: FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 500.0,
        }),
        end_anchor: Some(FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 600.0,
        })),
        note: None,
    });
    let data = serde_json::to_string(&fact).unwrap();
    let decoded: ControlFact = serde_json::from_str(&data).unwrap();
    assert_eq!(decoded, fact);
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
fn match_fact_anchor_reads_through_payload() {
    let control = ControlFact::PhaseStart(PhaseStartPayload {
        kind: PhaseKind::Regular,
        start_anchor: FactAnchor::Both {
            match_clock: MatchClock {
                elapsed_seconds: 0.0,
            },
            video_clock: VideoClock {
                elapsed_seconds: 60.0,
            },
        },
        end_anchor: FactAnchor::Both {
            match_clock: MatchClock {
                elapsed_seconds: 1800.0,
            },
            video_clock: VideoClock {
                elapsed_seconds: 1860.0,
            },
        },
    });
    // Swift 版は `recordedAt: .init()`（現在時刻）。コアもテストも now() を持たない方針のため
    // 固定値を使う（このテストで recorded_at の値は無関係）。
    let fact = MatchFact {
        id: Uuid::new_v4(),
        recorded_at: DateTime::from_timestamp(0, 0).unwrap(),
        payload: MatchFactPayload::Control(control),
    };
    if let FactAnchor::Both {
        match_clock,
        video_clock,
    } = fact.anchor()
    {
        assert_eq!(match_clock.elapsed_seconds, 0.0);
        assert_eq!(video_clock.elapsed_seconds, 60.0);
    } else {
        panic!("expected Both anchor");
    }
}

#[test]
fn phase_kind_has_only_two_cases() {
    assert_eq!(
        PhaseKind::ALL_CASES,
        [PhaseKind::Regular, PhaseKind::Shootout]
    );
}
