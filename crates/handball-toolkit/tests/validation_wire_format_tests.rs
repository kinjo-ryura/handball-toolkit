//! ADR 0002 のワイヤ形式 `(scope, code, params)` を固定する Rust 新設テスト（移植元なし）。
//!
//! Swift の `DomainValidationMessageTests.swift`（文言レイヤ）は移植対象外（ADR 0002 —
//! 文言はシェルが所有）。その責務のうちコアに残る「シェルの文言テーブルの lookup key
//! `(scope, code)` が安定していること」をここで担保する。code は Swift の case 名
//! そのままの安定契約であり、このテストの期待値の変更は breaking change を意味する。

use std::collections::BTreeSet;

use handball_toolkit::clock::FactAnchorKind;
use handball_toolkit::configuration::{MatchConfigurationKind, PhaseKind};
use handball_toolkit::facts::{PlayEventKind, StoppageKind};
use handball_toolkit::ids::{PlayerId, TeamId};
use handball_toolkit::validation::{
    ConfigurationValidationError, DomainValidationIssue, FactValidationError, MatchValidationError,
    TimelineValidationError,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn unit_case_serializes_as_scope_and_code_only() {
    let issue = DomainValidationIssue::Fact(FactValidationError::NegativeMatchClock);
    assert_eq!(
        serde_json::to_value(&issue).unwrap(),
        json!({ "scope": "fact", "code": "negativeMatchClock" })
    );
}

#[test]
fn payload_case_serializes_params_with_camel_case_keys() {
    let issue = DomainValidationIssue::Fact(FactValidationError::InvalidAnchorForConfiguration {
        configuration: MatchConfigurationKind::Timer,
        actual: FactAnchorKind::VideoClock,
        allowed: BTreeSet::from([FactAnchorKind::MatchClock]),
    });
    assert_eq!(
        serde_json::to_value(&issue).unwrap(),
        json!({
            "scope": "fact",
            "code": "invalidAnchorForConfiguration",
            "params": { "configuration": "timer", "actual": "videoClock", "allowed": ["matchClock"] }
        })
    );
}

#[test]
fn timeline_case_serializes_optional_phase_kind_hint() {
    let issue =
        DomainValidationIssue::Timeline(TimelineValidationError::PlayRecordedOutsidePhaseRange {
            kind: Some(PhaseKind::Regular),
        });
    assert_eq!(
        serde_json::to_value(&issue).unwrap(),
        json!({
            "scope": "timeline",
            "code": "playRecordedOutsidePhaseRange",
            "params": { "kind": "regular" }
        })
    );
}

#[test]
fn match_case_serializes_player_ids_deterministically() {
    let low = Uuid::from_u128(1);
    let high = Uuid::from_u128(2);
    let issue = DomainValidationIssue::Match(MatchValidationError::OverlappingRosterSelections {
        player_ids: BTreeSet::from([PlayerId(high), PlayerId(low)]),
    });
    assert_eq!(
        serde_json::to_value(&issue).unwrap(),
        json!({
            "scope": "match",
            "code": "overlappingRosterSelections",
            "params": { "playerIds": [low.to_string(), high.to_string()] }
        })
    );
}

#[test]
fn empty_video_external_id_keeps_swift_case_name() {
    // Swift case 名は `emptyVideoExternalID`（ID 大文字）。camelCase 機械変換とズレるため
    // 明示 rename を固定する（ADR 0002 の安定契約）。
    let issue =
        DomainValidationIssue::Configuration(ConfigurationValidationError::EmptyVideoExternalId);
    assert_eq!(
        serde_json::to_value(&issue).unwrap(),
        json!({ "scope": "configuration", "code": "emptyVideoExternalID" })
    );
}

#[test]
fn all_37_cases_round_trip_via_serde() {
    let team_id = TeamId(Uuid::from_u128(10));
    let player_id = PlayerId(Uuid::from_u128(11));
    let issues: Vec<DomainValidationIssue> = vec![
        // match: 3
        DomainValidationIssue::Match(MatchValidationError::SameTeamOnBothSides),
        DomainValidationIssue::Match(MatchValidationError::EmptyTitle),
        DomainValidationIssue::Match(MatchValidationError::OverlappingRosterSelections {
            player_ids: BTreeSet::from([player_id]),
        }),
        // configuration: 2
        DomainValidationIssue::Configuration(
            ConfigurationValidationError::NonPositivePhaseDuration { seconds: 0.0 },
        ),
        DomainValidationIssue::Configuration(ConfigurationValidationError::EmptyVideoExternalId),
        // fact: 20
        DomainValidationIssue::Fact(FactValidationError::NegativeMatchClock),
        DomainValidationIssue::Fact(FactValidationError::NegativeVideoClock),
        DomainValidationIssue::Fact(FactValidationError::InvalidAnchorForConfiguration {
            configuration: MatchConfigurationKind::Video,
            actual: FactAnchorKind::MatchClock,
            allowed: BTreeSet::from([FactAnchorKind::VideoClock, FactAnchorKind::Both]),
        }),
        DomainValidationIssue::Fact(FactValidationError::EmptyTitle),
        DomainValidationIssue::Fact(FactValidationError::EmptyNote),
        DomainValidationIssue::Fact(FactValidationError::DuplicatePrimaryAndRelatedPlayer),
        DomainValidationIssue::Fact(FactValidationError::MissingPlayerForPlayKind {
            kind: PlayEventKind::Goal,
        }),
        DomainValidationIssue::Fact(FactValidationError::FreeNoteHasNoContent),
        DomainValidationIssue::Fact(FactValidationError::PhaseStartMissingEndAnchor),
        DomainValidationIssue::Fact(FactValidationError::PhaseStartAnchorMismatch),
        DomainValidationIssue::Fact(FactValidationError::PhaseStartEndBeforeStart),
        DomainValidationIssue::Fact(FactValidationError::StoppageEndBeforeStart),
        DomainValidationIssue::Fact(FactValidationError::StoppageEndNilInVideoMode {
            kind: StoppageKind::Timeout,
        }),
        DomainValidationIssue::Fact(FactValidationError::StoppageEndPresentInTimerMode {
            kind: StoppageKind::Pause,
        }),
        DomainValidationIssue::Fact(FactValidationError::TimeoutHasNote),
        DomainValidationIssue::Fact(FactValidationError::EmptyStoppageNote),
        DomainValidationIssue::Fact(FactValidationError::UnknownTeamReference { team_id }),
        DomainValidationIssue::Fact(FactValidationError::UnknownPlayerReference { player_id }),
        DomainValidationIssue::Fact(FactValidationError::PlayerTeamMismatch { player_id, team_id }),
        DomainValidationIssue::Fact(FactValidationError::RelatedPlayerTeamMismatch {
            player_id,
            team_id,
        }),
        // timeline: 12
        DomainValidationIssue::Timeline(TimelineValidationError::TimerWithFactsMissingPhaseStart),
        DomainValidationIssue::Timeline(TimelineValidationError::VideoWithFactsMissingPhaseStart),
        DomainValidationIssue::Timeline(TimelineValidationError::VideoHighlightContainsPhaseStart),
        DomainValidationIssue::Timeline(TimelineValidationError::VideoHighlightContainsStoppage),
        DomainValidationIssue::Timeline(TimelineValidationError::VideoHighlightMissingTitle),
        DomainValidationIssue::Timeline(TimelineValidationError::PlayRecordedOutsidePhaseRange {
            kind: None,
        }),
        DomainValidationIssue::Timeline(TimelineValidationError::PlayRecordedInsideStoppage),
        DomainValidationIssue::Timeline(TimelineValidationError::DuplicateShootout),
        DomainValidationIssue::Timeline(TimelineValidationError::ShootoutNotLast),
        DomainValidationIssue::Timeline(
            TimelineValidationError::PhaseStartNotContinuousFromPrevious,
        ),
        DomainValidationIssue::Timeline(TimelineValidationError::StoppagesOverlap),
        DomainValidationIssue::Timeline(TimelineValidationError::StoppageOutsidePhaseRange),
    ];
    assert_eq!(issues.len(), 37);
    for issue in &issues {
        let data = serde_json::to_string(issue).unwrap();
        let decoded: DomainValidationIssue = serde_json::from_str(&data).unwrap();
        assert_eq!(&decoded, issue, "round trip failed: {data}");
    }
}
