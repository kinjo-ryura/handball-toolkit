//! 移植元: `Tests/RecorderDomainTests/FactLogValidatorTests.swift`。
//!
//! （Swift の suite static 群は Rust では各テスト内の `ctx()` ローカル値。
//! ID の一貫性は 1 テスト内で閉じる。）

use chrono::{DateTime, Utc};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
use handball_toolkit::ids::TeamId;
use handball_toolkit::validation::{DomainValidationIssue, TimelineValidationError};
use handball_toolkit::validators::validate_fact_log;
use uuid::Uuid;

fn recorded_at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

struct Ctx {
    home_id: Uuid,
    timer_match: Match,
    video_match: Match,
    highlight_match: Match,
}

fn make_match(
    home_id: TeamId,
    away_id: TeamId,
    configuration: MatchConfiguration,
    title: Option<&str>,
) -> Match {
    Match {
        id: Uuid::new_v4(),
        title: title.map(str::to_owned),
        date: recorded_at(),
        home_team_id: home_id,
        away_team_id: away_id,
        configuration,
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    }
}

fn ctx() -> Ctx {
    let home_id = Uuid::new_v4();
    let away_id = Uuid::new_v4();
    let youtube = |id: &str| VideoSource {
        provider: VideoProvider::Youtube,
        external_id: id.to_owned(),
    };
    Ctx {
        home_id,
        timer_match: make_match(
            home_id,
            away_id,
            MatchConfiguration::Timer {
                phase_duration_seconds: 1800.0,
            },
            Some("Test"),
        ),
        video_match: make_match(
            home_id,
            away_id,
            MatchConfiguration::Video(youtube("abc")),
            Some("Test"),
        ),
        highlight_match: make_match(
            home_id,
            away_id,
            MatchConfiguration::VideoHighlight(youtube("abc")),
            Some("ハイライト集"),
        ),
    }
}

fn timer_phase_start(start: f64, end: f64, kind: PhaseKind) -> MatchFact {
    MatchFact {
        id: Uuid::new_v4(),
        recorded_at: recorded_at(),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: start,
            }),
            end_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: end,
            }),
        })),
    }
}

fn video_phase_start(start: f64, end: f64) -> MatchFact {
    MatchFact {
        id: Uuid::new_v4(),
        recorded_at: recorded_at(),
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

fn video_stoppage(start: f64, end: Option<f64>, kind: StoppageKind) -> MatchFact {
    MatchFact {
        id: Uuid::new_v4(),
        recorded_at: recorded_at(),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: start,
            }),
            end_anchor: end.map(|secs| {
                FactAnchor::VideoClock(VideoClock {
                    elapsed_seconds: secs,
                })
            }),
            note: None,
        })),
    }
}

fn timer_play(kind: PlayEventKind, team: Option<TeamId>, secs: f64) -> MatchFact {
    MatchFact {
        id: Uuid::new_v4(),
        recorded_at: recorded_at(),
        payload: MatchFactPayload::Play(PlayFact {
            kind,
            team_id: team,
            player_id: team.map(|_| Uuid::new_v4()),
            related_player_id: None,
            anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: secs,
            }),
            title: None,
            note: None,
        }),
    }
}

fn video_play(kind: PlayEventKind, team: Option<TeamId>, video_secs: f64) -> MatchFact {
    MatchFact {
        id: Uuid::new_v4(),
        recorded_at: recorded_at(),
        payload: MatchFactPayload::Play(PlayFact {
            kind,
            team_id: team,
            player_id: team.map(|_| Uuid::new_v4()),
            related_player_id: None,
            anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: video_secs,
            }),
            title: None,
            note: None,
        }),
    }
}

// ── happy path ──

#[test]
fn empty_log_is_valid() {
    let c = ctx();
    let issues = validate_fact_log(&[], &c.timer_match);
    assert!(issues.is_empty());
}

#[test]
fn empty_log_is_valid_for_video() {
    let c = ctx();
    let issues = validate_fact_log(&[], &c.video_match);
    assert!(issues.is_empty());
}

#[test]
fn clean_timer_match_is_valid() {
    let c = ctx();
    let facts = vec![
        timer_phase_start(0.0, 1800.0, PhaseKind::Regular),
        timer_play(PlayEventKind::Goal, Some(c.home_id), 600.0),
        timer_phase_start(1800.0, 3600.0, PhaseKind::Regular),
    ];
    assert!(validate_fact_log(&facts, &c.timer_match).is_empty());
}

#[test]
fn clean_video_match_is_valid() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_play(PlayEventKind::Goal, Some(c.home_id), 600.0),
    ];
    assert!(validate_fact_log(&facts, &c.video_match).is_empty());
}

// ── R3 / R5: PhaseStart 必須 (fact 1 件以上時) ──

#[test]
fn timer_with_facts_but_no_phase_start_is_blocking() {
    let c = ctx();
    let facts = vec![timer_play(PlayEventKind::FreeNote, None, 100.0)];
    let issues = validate_fact_log(&facts, &c.timer_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::TimerWithFactsMissingPhaseStart
    )));
}

#[test]
fn video_with_facts_but_no_phase_start_is_blocking() {
    let c = ctx();
    let facts = vec![video_play(PlayEventKind::FreeNote, None, 100.0)];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::VideoWithFactsMissingPhaseStart
    )));
}

// ── R6 / R9: videoHighlight + PhaseStart / Stoppage 禁止 ──

#[test]
fn video_highlight_with_phase_start_is_blocking() {
    let c = ctx();
    let facts = vec![video_phase_start(0.0, 1800.0)];
    let issues = validate_fact_log(&facts, &c.highlight_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::VideoHighlightContainsPhaseStart
    )));
}

#[test]
fn video_highlight_with_stoppage_is_blocking() {
    let c = ctx();
    let facts = vec![video_stoppage(60.0, Some(120.0), StoppageKind::Timeout)];
    let issues = validate_fact_log(&facts, &c.highlight_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::VideoHighlightContainsStoppage
    )));
}

#[test]
fn video_highlight_with_play_only_is_accepted() {
    let c = ctx();
    let facts = vec![video_play(PlayEventKind::FreeNote, None, 30.0)];
    let issues = validate_fact_log(&facts, &c.highlight_match);
    assert!(issues.is_empty());
}

// ── R11: videoHighlight + title 必須 ──

#[test]
fn video_highlight_without_title_is_blocking() {
    let c = ctx();
    let match_ = make_match(
        c.home_id,
        Uuid::new_v4(),
        MatchConfiguration::VideoHighlight(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "abc".to_owned(),
        }),
        None,
    );
    let facts = vec![video_play(PlayEventKind::FreeNote, None, 30.0)];
    let issues = validate_fact_log(&facts, &match_);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::VideoHighlightMissingTitle
    )));
}

#[test]
fn video_highlight_with_empty_title_is_blocking() {
    let c = ctx();
    let match_ = make_match(
        c.home_id,
        Uuid::new_v4(),
        MatchConfiguration::VideoHighlight(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "abc".to_owned(),
        }),
        Some("   "),
    );
    let issues = validate_fact_log(&[], &match_);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::VideoHighlightMissingTitle
    )));
}

// ── R7: video play outside phase range ──

#[test]
fn video_play_outside_phase_range_is_blocking() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_phase_start(2700.0, 4500.0),
        // halftime (1800-2700) に play
        video_play(PlayEventKind::FreeNote, None, 2000.0),
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    let hit = issues.iter().any(|issue| {
        matches!(
            issue,
            DomainValidationIssue::Timeline(
                TimelineValidationError::PlayRecordedOutsidePhaseRange { .. }
            )
        )
    });
    assert!(hit);
}

#[test]
fn video_play_inside_phase_range_is_accepted() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_play(PlayEventKind::Goal, Some(c.home_id), 600.0),
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(issues.is_empty());
}

// ── R8: video Stoppage range 内 play ──

#[test]
fn video_play_inside_stoppage_is_blocking() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_stoppage(500.0, Some(600.0), StoppageKind::Timeout),
        video_play(PlayEventKind::FreeNote, None, 550.0),
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::PlayRecordedInsideStoppage
    )));
}

// ── shootout ──

#[test]
fn duplicate_shootout_is_blocking() {
    let c = ctx();
    let facts = vec![
        timer_phase_start(0.0, 1800.0, PhaseKind::Regular),
        timer_phase_start(1800.0, 1800.0, PhaseKind::Shootout),
        timer_phase_start(1800.0, 1800.0, PhaseKind::Shootout),
    ];
    let issues = validate_fact_log(&facts, &c.timer_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::DuplicateShootout
    )));
}

#[test]
fn shootout_not_last_is_blocking() {
    let c = ctx();
    let facts = vec![
        timer_phase_start(0.0, 1800.0, PhaseKind::Regular),
        timer_phase_start(1800.0, 1800.0, PhaseKind::Shootout),
        timer_phase_start(1800.0, 3600.0, PhaseKind::Regular),
    ];
    let issues = validate_fact_log(&facts, &c.timer_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::ShootoutNotLast
    )));
}

#[test]
fn shootout_as_last_phase_is_valid() {
    let c = ctx();
    let facts = vec![
        timer_phase_start(0.0, 1800.0, PhaseKind::Regular),
        timer_phase_start(1800.0, 3600.0, PhaseKind::Regular),
        timer_phase_start(3600.0, 3600.0, PhaseKind::Shootout),
    ];
    let issues = validate_fact_log(&facts, &c.timer_match);
    assert!(issues.is_empty());
}

// ── timer regular 連続性 ──

#[test]
fn timer_phase_gap_is_blocking() {
    let c = ctx();
    let facts = vec![
        timer_phase_start(0.0, 1800.0, PhaseKind::Regular),
        timer_phase_start(1900.0, 3700.0, PhaseKind::Regular), // gap: 1800 → 1900
    ];
    let issues = validate_fact_log(&facts, &c.timer_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::PhaseStartNotContinuousFromPrevious
    )));
}

#[test]
fn timer_phase_overlap_is_blocking() {
    let c = ctx();
    let facts = vec![
        timer_phase_start(0.0, 1800.0, PhaseKind::Regular),
        timer_phase_start(1700.0, 3500.0, PhaseKind::Regular), // overlap: 1800 > 1700
    ];
    let issues = validate_fact_log(&facts, &c.timer_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::PhaseStartNotContinuousFromPrevious
    )));
}

#[test]
fn video_phase_gap_is_accepted() {
    // video モードでは continuity check は不要 (SegmentResolver が構造的に保証)
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_phase_start(2700.0, 4500.0), // ハーフタイム gap は OK
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(!issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::PhaseStartNotContinuousFromPrevious
    )));
}

// ── Stoppage 重複 ──

#[test]
fn overlapping_stoppages_are_blocking() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_stoppage(100.0, Some(200.0), StoppageKind::Timeout),
        video_stoppage(150.0, Some(250.0), StoppageKind::Pause),
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::StoppagesOverlap
    )));
}

#[test]
fn adjacent_stoppages_are_accepted() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_stoppage(100.0, Some(200.0), StoppageKind::Timeout),
        video_stoppage(200.0, Some(300.0), StoppageKind::Timeout),
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(!issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::StoppagesOverlap
    )));
}

#[test]
fn stoppage_outside_phase_is_blocking() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        // halftime 中の Stoppage
        video_stoppage(2000.0, Some(2100.0), StoppageKind::Pause),
        video_phase_start(2700.0, 4500.0),
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::StoppageOutsidePhaseRange
    )));
}
