//! 移植元: `Tests/RecorderDomainTests/FactValidatorTests.swift`。
//!
//! （Swift の suite static 群は Rust では各テスト内の `ctx()` ローカル値。
//! ID の一貫性は 1 テスト内で閉じる。）

use std::collections::{BTreeMap, BTreeSet};

use chrono::DateTime;
use handball_toolkit::clock::{FactAnchor, FactAnchorKind, MatchClock, VideoClock};
use handball_toolkit::configuration::{
    MatchConfiguration, MatchConfigurationKind, PhaseKind, VideoProvider, VideoSource,
};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
use handball_toolkit::ids::{FactId, PlayerId, TeamId};
use handball_toolkit::validation::{DomainValidationIssue, FactValidationError};
use handball_toolkit::validators::{
    RosterContext, validate_control_fact, validate_match_fact, validate_play_fact,
};
use uuid::Uuid;

struct Ctx {
    home_id: TeamId,
    player1: PlayerId,
    player2: PlayerId,
    timer_config: MatchConfiguration,
    video_config: MatchConfiguration,
    roster: RosterContext,
}

fn ctx() -> Ctx {
    let home_id = TeamId(Uuid::new_v4());
    let away_id = TeamId(Uuid::new_v4());
    let player1 = PlayerId(Uuid::new_v4());
    let player2 = PlayerId(Uuid::new_v4());
    Ctx {
        home_id,
        player1,
        player2,
        timer_config: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        video_config: MatchConfiguration::Video(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "abc".to_owned(),
        }),
        roster: RosterContext {
            home_team_id: home_id,
            away_team_id: away_id,
            player_team_lookup: BTreeMap::from([(player1, home_id), (player2, away_id)]),
            known_player_ids: None,
        },
    }
}

fn mc(secs: f64) -> FactAnchor {
    FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: secs,
    })
}

fn vc(secs: f64) -> FactAnchor {
    FactAnchor::VideoClock(VideoClock {
        elapsed_seconds: secs,
    })
}

fn both(match_secs: f64, video_secs: f64) -> FactAnchor {
    FactAnchor::Both {
        match_clock: MatchClock {
            elapsed_seconds: match_secs,
        },
        video_clock: VideoClock {
            elapsed_seconds: video_secs,
        },
    }
}

fn play_base(kind: PlayEventKind, anchor: FactAnchor) -> PlayFact {
    PlayFact {
        kind,
        team_id: None,
        player_id: None,
        related_player_id: None,
        anchor,
        title: None,
        note: None,
    }
}

// ── PlayFact (timer) ──

#[test]
fn timer_goal_is_valid() {
    let c = ctx();
    let fact = PlayFact {
        team_id: Some(c.home_id),
        player_id: Some(c.player1),
        ..play_base(PlayEventKind::Goal, mc(600.0))
    };
    assert!(validate_play_fact(&fact, &c.timer_config, &c.roster).is_empty());
}

#[test]
fn timer_play_with_video_anchor_is_rejected() {
    let c = ctx();
    let fact = PlayFact {
        team_id: Some(c.home_id),
        player_id: Some(c.player1),
        ..play_base(PlayEventKind::Goal, vc(600.0))
    };
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    let hit = issues.iter().any(|issue| {
        matches!(
            issue,
            DomainValidationIssue::Fact(FactValidationError::InvalidAnchorForConfiguration {
                configuration,
                actual,
                allowed,
            }) if *configuration == MatchConfigurationKind::Timer
                && *actual == FactAnchorKind::VideoClock
                && *allowed == BTreeSet::from([FactAnchorKind::MatchClock])
        )
    });
    assert!(hit);
}

#[test]
fn video_play_with_match_anchor_is_rejected() {
    let c = ctx();
    let fact = PlayFact {
        team_id: Some(c.home_id),
        player_id: Some(c.player1),
        ..play_base(PlayEventKind::Goal, mc(600.0))
    };
    let issues = validate_play_fact(&fact, &c.video_config, &c.roster);
    let hit = issues.iter().any(|issue| {
        matches!(
            issue,
            DomainValidationIssue::Fact(FactValidationError::InvalidAnchorForConfiguration {
                configuration,
                actual,
                allowed,
            }) if *configuration == MatchConfigurationKind::Video
                && *actual == FactAnchorKind::MatchClock
                && *allowed == BTreeSet::from([FactAnchorKind::VideoClock, FactAnchorKind::Both])
        )
    });
    assert!(hit);
}

#[test]
fn negative_match_clock_is_rejected() {
    let c = ctx();
    let fact = PlayFact {
        team_id: Some(c.home_id),
        player_id: Some(c.player1),
        ..play_base(PlayEventKind::Goal, mc(-1.0))
    };
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::NegativeMatchClock
    )));
}

#[test]
fn goal_without_player_is_rejected() {
    let c = ctx();
    let fact = play_base(PlayEventKind::Goal, mc(0.0));
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::MissingPlayerForPlayKind {
            kind: PlayEventKind::Goal
        }
    )));
}

#[test]
fn goal_without_team_is_allowed_when_player_known() {
    // teamID は optional (playerID から導出可能)。
    let c = ctx();
    let fact = PlayFact {
        player_id: Some(c.player1),
        ..play_base(PlayEventKind::Goal, mc(0.0))
    };
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    assert!(issues.is_empty());
}

#[test]
fn anchor_only_free_note_is_valid() {
    // freeNote は teamID / playerID / note / title すべて optional (anchor だけのマーカー freeNote も valid)。
    let c = ctx();
    let fact = play_base(PlayEventKind::FreeNote, mc(0.0));
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    assert!(issues.is_empty());
}

#[test]
fn duplicate_primary_and_related_player_is_rejected() {
    let c = ctx();
    let fact = PlayFact {
        team_id: Some(c.home_id),
        player_id: Some(c.player1),
        related_player_id: Some(c.player1),
        ..play_base(PlayEventKind::Goal, mc(0.0))
    };
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::DuplicatePrimaryAndRelatedPlayer
    )));
}

#[test]
fn player_team_mismatch_is_reported() {
    // player2 は awayID 所属だが、teamID に homeID を渡す
    let c = ctx();
    let fact = PlayFact {
        team_id: Some(c.home_id),
        player_id: Some(c.player2),
        ..play_base(PlayEventKind::Goal, mc(0.0))
    };
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    let hit = issues.iter().any(|issue| {
        matches!(
            issue,
            DomainValidationIssue::Fact(FactValidationError::PlayerTeamMismatch {
                player_id,
                team_id,
            }) if *player_id == c.player2 && *team_id == c.home_id
        )
    });
    assert!(hit);
}

#[test]
fn unknown_team_reference_is_reported() {
    let c = ctx();
    let foreign_team = TeamId(Uuid::new_v4());
    let fact = PlayFact {
        team_id: Some(foreign_team),
        player_id: Some(c.player1),
        ..play_base(PlayEventKind::Goal, mc(0.0))
    };
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    let hit = issues.iter().any(|issue| {
        matches!(
            issue,
            DomainValidationIssue::Fact(FactValidationError::UnknownTeamReference { .. })
        )
    });
    assert!(hit);
}

#[test]
fn empty_title_string_is_rejected() {
    let c = ctx();
    let fact = PlayFact {
        title: Some("   ".to_owned()),
        ..play_base(PlayEventKind::FreeNote, mc(0.0))
    };
    let issues = validate_play_fact(&fact, &c.timer_config, &c.roster);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::EmptyTitle
    )));
}

// ── PhaseStart (timer) ──

#[test]
fn valid_timer_phase_start_is_accepted() {
    let c = ctx();
    let fact = ControlFact::PhaseStart(PhaseStartPayload {
        kind: PhaseKind::Regular,
        start_anchor: mc(0.0),
        end_anchor: mc(1800.0),
    });
    let issues = validate_control_fact(&fact, &c.timer_config);
    assert!(issues.is_empty());
}

#[test]
fn phase_start_end_before_start_is_rejected() {
    let c = ctx();
    let fact = ControlFact::PhaseStart(PhaseStartPayload {
        kind: PhaseKind::Regular,
        start_anchor: mc(1800.0),
        end_anchor: mc(0.0),
    });
    let issues = validate_control_fact(&fact, &c.timer_config);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::PhaseStartEndBeforeStart
    )));
}

#[test]
fn phase_start_anchor_mismatch_is_rejected() {
    let c = ctx();
    let fact = ControlFact::PhaseStart(PhaseStartPayload {
        kind: PhaseKind::Regular,
        start_anchor: mc(0.0),
        end_anchor: vc(1800.0),
    });
    let issues = validate_control_fact(&fact, &c.timer_config);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::PhaseStartAnchorMismatch
    )));
}

// ── PhaseStart (video) ──

#[test]
fn video_phase_start_with_video_clock_is_accepted() {
    let c = ctx();
    let fact = ControlFact::PhaseStart(PhaseStartPayload {
        kind: PhaseKind::Regular,
        start_anchor: vc(0.0),
        end_anchor: vc(1800.0),
    });
    let issues = validate_control_fact(&fact, &c.video_config);
    assert!(issues.is_empty());
}

#[test]
fn video_phase_start_with_both_is_accepted() {
    let c = ctx();
    let fact = ControlFact::PhaseStart(PhaseStartPayload {
        kind: PhaseKind::Regular,
        start_anchor: both(0.0, 100.0),
        end_anchor: both(1800.0, 1900.0),
    });
    let issues = validate_control_fact(&fact, &c.video_config);
    assert!(issues.is_empty());
}

// ── Stoppage (timer) ──

#[test]
fn timer_stoppage_without_end_is_accepted() {
    let c = ctx();
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Timeout,
        start_anchor: mc(600.0),
        end_anchor: None,
        note: None,
    });
    let issues = validate_control_fact(&fact, &c.timer_config);
    assert!(issues.is_empty());
}

#[test]
fn timer_stoppage_with_end_is_rejected() {
    let c = ctx();
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Timeout,
        start_anchor: mc(600.0),
        end_anchor: Some(mc(660.0)),
        note: None,
    });
    let issues = validate_control_fact(&fact, &c.timer_config);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::StoppageEndPresentInTimerMode {
            kind: StoppageKind::Timeout
        }
    )));
}

// ── Stoppage (video) ──

#[test]
fn video_stoppage_without_end_is_rejected() {
    let c = ctx();
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Timeout,
        start_anchor: vc(600.0),
        end_anchor: None,
        note: None,
    });
    let issues = validate_control_fact(&fact, &c.video_config);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::StoppageEndNilInVideoMode {
            kind: StoppageKind::Timeout
        }
    )));
}

#[test]
fn video_stoppage_with_end_is_accepted() {
    let c = ctx();
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Timeout,
        start_anchor: vc(600.0),
        end_anchor: Some(vc(660.0)),
        note: None,
    });
    let issues = validate_control_fact(&fact, &c.video_config);
    assert!(issues.is_empty());
}

#[test]
fn video_stoppage_end_before_start_is_rejected() {
    let c = ctx();
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Timeout,
        start_anchor: vc(660.0),
        end_anchor: Some(vc(600.0)),
        note: None,
    });
    let issues = validate_control_fact(&fact, &c.video_config);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::StoppageEndBeforeStart
    )));
}

#[test]
fn timeout_with_note_is_rejected() {
    let c = ctx();
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Timeout,
        start_anchor: vc(600.0),
        end_anchor: Some(vc(660.0)),
        note: Some("戦術タイムアウト".to_owned()),
    });
    let issues = validate_control_fact(&fact, &c.video_config);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::TimeoutHasNote
    )));
}

#[test]
fn pause_with_note_is_accepted() {
    let c = ctx();
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Pause,
        start_anchor: vc(600.0),
        end_anchor: Some(vc(660.0)),
        note: Some("怪我対応".to_owned()),
    });
    let issues = validate_control_fact(&fact, &c.video_config);
    assert!(issues.is_empty());
}

#[test]
fn empty_pause_note_is_rejected() {
    let c = ctx();
    let fact = ControlFact::Stoppage(StoppagePayload {
        kind: StoppageKind::Pause,
        start_anchor: vc(600.0),
        end_anchor: Some(vc(660.0)),
        note: Some("  ".to_owned()),
    });
    let issues = validate_control_fact(&fact, &c.video_config);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::EmptyStoppageNote
    )));
}

// ── MatchFact dispatch ──

#[test]
fn match_fact_dispatches_to_play_validation() {
    let c = ctx();
    let bad = play_base(PlayEventKind::Goal, mc(0.0));
    let fact = MatchFact {
        id: FactId(Uuid::new_v4()),
        recorded_at: DateTime::from_timestamp(0, 0).unwrap(),
        payload: MatchFactPayload::Play(bad),
    };
    let issues = validate_match_fact(&fact, &c.timer_config, &c.roster);
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::MissingPlayerForPlayKind {
            kind: PlayEventKind::Goal
        }
    )));
}
