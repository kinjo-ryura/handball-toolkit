//! 移植元: `Tests/RecorderDomainTests/TimelineProjectionTests.swift`。
//!
//! TimelineProjection / SegmentResolver の MVP テスト。
//! （Swift の suite static `homeID` / `awayID` は、Rust では各テスト内のローカル乱数 ID。
//! ID の一貫性は 1 テスト内で閉じているため意味論は同じ。）

mod fixtures;

use fixtures::{
    make_timer_match, make_video_match, phase_start_both, play_both, timer_phase, timer_play,
    video_only_phase, video_play,
};
use handball_toolkit::clock::{MatchClock, VideoClock};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::ids::{FactId, TeamId};
use handball_toolkit::projection::{SegmentResolver, TimelineProjection};
use uuid::Uuid;

/// Swift の `videoPhaseStartBoth(matchStart:matchEnd:videoStart:videoEnd:)` 相当。
fn video_phase_start_both(
    match_start: f64,
    match_end: f64,
    video_start: f64,
    video_end: f64,
) -> handball_toolkit::facts::MatchFact {
    phase_start_both(
        PhaseKind::Regular,
        match_start,
        video_start,
        match_end,
        video_end,
    )
}

// ── 構築 ──

#[test]
fn empty_facts_builds_empty_projection() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let projection = TimelineProjection::build(&make_video_match(home, away), &[]);
    assert!(projection.resolved_facts.is_empty());
    assert!(projection.resolver.phases.is_empty());
}

#[test]
fn phase_start_creates_phase_entry() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![video_only_phase(720.0, 2520.0)];
    let projection = TimelineProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(projection.resolver.phases.len(), 1);
    let phase = projection.resolver.phases.first().unwrap();
    assert_eq!(phase.kind, PhaseKind::Regular);
    assert_eq!(phase.video_elapsed_start, Some(720.0));
    assert_eq!(phase.video_elapsed_end, Some(2520.0));
}

// ── resolveClocks for play fact ──

#[test]
fn video_play_resolves_match_clock_via_phase() {
    // 動画 12:00 で前半開始 (matchClock=0)、動画 12:10 ゴール (matchClock=10s 相当)
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        video_phase_start_both(0.0, 1800.0, 720.0, 2520.0),
        video_play(home, 730.0),
    ];
    let projection = TimelineProjection::build(&make_video_match(home, away), &facts);
    let goal = projection.resolved_facts.last().unwrap();
    assert_eq!(
        goal.resolved_video_clock.map(|c| c.elapsed_seconds),
        Some(730.0)
    );
    assert_eq!(
        goal.resolved_match_clock.map(|c| c.elapsed_seconds),
        Some(10.0)
    );
}

#[test]
fn both_anchor_is_not_overwritten_by_projection() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        video_phase_start_both(0.0, 1800.0, 720.0, 2520.0),
        play_both(home, 50.0, 800.0),
    ];
    let projection = TimelineProjection::build(&make_video_match(home, away), &facts);
    let last = projection.resolved_facts.last().unwrap();
    // anchor の明示値そのまま (projection で上書きしない)
    assert_eq!(
        last.resolved_match_clock.map(|c| c.elapsed_seconds),
        Some(50.0)
    );
    assert_eq!(
        last.resolved_video_clock.map(|c| c.elapsed_seconds),
        Some(800.0)
    );
}

#[test]
fn match_clock_only_play_in_timer_mode_resolves_as_is() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        timer_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        timer_play(home, 600.0),
    ];
    let projection = TimelineProjection::build(&make_timer_match(home, away), &facts);
    let last = projection.resolved_facts.last().unwrap();
    assert_eq!(
        last.resolved_match_clock.map(|c| c.elapsed_seconds),
        Some(600.0)
    );
}

// ── SegmentResolver direct API ──

#[test]
fn resolver_converts_video_to_match_inside_regular_phase() {
    let facts = vec![video_phase_start_both(0.0, 1800.0, 720.0, 2520.0)];
    let resolver = SegmentResolver::build(&facts);
    let mc = resolver.resolve_match_clock(VideoClock {
        elapsed_seconds: 750.0,
    });
    assert_eq!(mc.map(|c| c.elapsed_seconds), Some(30.0));
}

#[test]
fn resolver_converts_match_to_video_inside_regular_phase() {
    let facts = vec![video_phase_start_both(0.0, 1800.0, 720.0, 2520.0)];
    let resolver = SegmentResolver::build(&facts);
    let v = resolver.resolve_video_clock(MatchClock {
        elapsed_seconds: 30.0,
    });
    assert_eq!(v.map(|c| c.elapsed_seconds), Some(750.0));
}

#[test]
fn resolver_returns_none_outside_any_phase() {
    let facts = vec![video_phase_start_both(0.0, 1800.0, 720.0, 2520.0)];
    let resolver = SegmentResolver::build(&facts);
    let mc = resolver.resolve_match_clock(VideoClock {
        elapsed_seconds: 100.0,
    });
    assert_eq!(mc, None);
}

// ── PhaseKind / phaseIndex ──

#[test]
fn phase_kind_is_regular_inside_phase() {
    let facts = vec![video_phase_start_both(0.0, 1800.0, 720.0, 2520.0)];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(resolver.phase_kind(30.0), Some(PhaseKind::Regular));
    assert_eq!(resolver.phase_kind(5000.0), None);
}

#[test]
fn phase_index_counts_regular_only() {
    let facts = vec![
        video_phase_start_both(0.0, 1800.0, 720.0, 2520.0),
        video_phase_start_both(1800.0, 3600.0, 3000.0, 4800.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(resolver.phase_index(30.0), Some(0));
    assert_eq!(resolver.phase_index(2000.0), Some(1));
}

/// timer mode の play は video 派生を持たない (resolved_video_clock == None)。
#[test]
fn timer_play_has_none_resolved_video_clock() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        timer_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        timer_play(home, 600.0),
    ];
    let projection = TimelineProjection::build(&make_timer_match(home, away), &facts);
    let last = projection.resolved_facts.last().unwrap();
    assert_eq!(
        last.resolved_match_clock.map(|c| c.elapsed_seconds),
        Some(600.0)
    );
    assert_eq!(last.resolved_video_clock, None);
}

/// resolved_fact(id) は id 一致で引け、未知 id では None。
#[test]
fn resolved_fact_lookup_by_id() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let goal = video_play(home, 730.0);
    let goal_id = goal.id;
    let facts = vec![video_phase_start_both(0.0, 1800.0, 720.0, 2520.0), goal];
    let projection = TimelineProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(
        projection.resolved_fact(goal_id).map(|r| r.fact.id),
        Some(goal_id)
    );
    assert!(projection.resolved_fact(FactId(Uuid::new_v4())).is_none());
}
