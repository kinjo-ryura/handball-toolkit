//! 移植元: `Tests/RecorderDomainTests/ScoreProgressionProjectionTests.swift`。

mod fixtures;

use fixtures::{
    make_timer_match, make_video_match, phase_start_both, phase_start_match, play_at_match,
    video_play,
};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::facts::PlayEventKind;
use handball_toolkit::ids::{PlayerId, TeamId};
use handball_toolkit::projection::ScoreProgressionProjection;
use uuid::Uuid;

// ── None 条件 ──

#[test]
fn no_phases_returns_none() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    assert!(ScoreProgressionProjection::build(&make_timer_match(home, away), &[]).is_none());
}

#[test]
fn phase_but_no_goals_returns_none() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![phase_start_match(PhaseKind::Regular, 0.0, 1800.0)];
    assert!(ScoreProgressionProjection::build(&make_timer_match(home, away), &facts).is_none());
}

// ── step-doubling 構造 ──

/// 単一 goal → 先頭(0,0) + goal 前/後 の 2 点 + 末尾(total) = 4 点。
#[test]
fn single_goal_produces_doubled_points() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0),
        play_at_match(PlayEventKind::Goal, home, PlayerId(Uuid::new_v4()), 100.0),
    ];
    let p = ScoreProgressionProjection::build(&make_timer_match(home, away), &facts);
    assert!(p.is_some());
    let p = p.unwrap();
    assert_eq!(p.points.len(), 4);
    let secs: Vec<f64> = p.points.iter().map(|pt| pt.cumulative_seconds).collect();
    assert_eq!(secs, vec![0.0, 100.0, 100.0, 1800.0]);
    let diffs: Vec<i64> = p.points.iter().map(|pt| pt.diff()).collect();
    assert_eq!(diffs, vec![0, 0, -1, -1]); // home goal → away − home = -1
    assert_eq!(p.total_seconds, 1800.0);
    assert_eq!(p.max_abs_diff, 1);
    assert_eq!(p.phase_spans.len(), 1);
    assert_eq!(p.phase_spans[0].regular_index, 0);
    assert_eq!(p.phase_spans[0].start_seconds, 0.0);
    assert_eq!(p.phase_spans[0].end_seconds, 1800.0);
}

/// 複数 goal で diff が階段状に動く (away, away, home → +1, +2, +1)。
#[test]
fn multiple_goals_step_diff() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0),
        play_at_match(PlayEventKind::Goal, away, PlayerId(Uuid::new_v4()), 100.0),
        play_at_match(PlayEventKind::Goal, away, PlayerId(Uuid::new_v4()), 200.0),
        play_at_match(PlayEventKind::Goal, home, PlayerId(Uuid::new_v4()), 300.0),
    ];
    let p = ScoreProgressionProjection::build(&make_timer_match(home, away), &facts).unwrap();
    assert_eq!(p.points.len(), 8); // 先頭1 + 3*2 + 末尾1
    let diffs: Vec<i64> = p.points.iter().map(|pt| pt.diff()).collect();
    assert_eq!(diffs, vec![0, 0, 1, 1, 2, 2, 1, 1]);
    assert_eq!(p.max_abs_diff, 2);
}

// ── phase 区間 ──

#[test]
fn two_phases_produce_two_spans() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0),
        phase_start_match(PhaseKind::Regular, 1800.0, 3600.0),
        play_at_match(PlayEventKind::Goal, home, PlayerId(Uuid::new_v4()), 100.0),
        play_at_match(PlayEventKind::Goal, away, PlayerId(Uuid::new_v4()), 2000.0),
    ];
    let p = ScoreProgressionProjection::build(&make_timer_match(home, away), &facts).unwrap();
    let indexes: Vec<usize> = p.phase_spans.iter().map(|s| s.regular_index).collect();
    assert_eq!(indexes, vec![0, 1]);
    assert_eq!(p.phase_spans[1].start_seconds, 1800.0);
    assert_eq!(p.total_seconds, 3600.0);
}

// ── Q5 特殊ケースの固定 ──

/// shootout goal は degenerate clock のため最終 regular phase 終端 (totalSeconds) に重なる。
#[test]
fn shootout_goals_pile_at_last_regular_end() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0),
        phase_start_match(PhaseKind::Shootout, 1800.0, 1800.0),
        play_at_match(PlayEventKind::Goal, home, PlayerId(Uuid::new_v4()), 100.0),
        play_at_match(PlayEventKind::Goal, away, PlayerId(Uuid::new_v4()), 1800.0), // shootout goal (degenerate clock)
    ];
    let p = ScoreProgressionProjection::build(&make_timer_match(home, away), &facts).unwrap();
    assert_eq!(p.phase_spans.len(), 1); // spans は regular のみ
    assert_eq!(p.total_seconds, 1800.0);
    // shootout の away goal は degenerate clock のため最終 regular 終端 (1800) に乗り、1-1 にする。
    assert!(
        p.points
            .iter()
            .any(|pt| pt.cumulative_seconds == 1800.0 && pt.home_score == 1 && pt.away_score == 1)
    );
}

/// matchClock を解決できない goal (video 位置が phase 区間外) は除外される。
#[test]
fn unresolvable_goal_excluded() {
    let (home, away) = (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()));
    let facts = vec![
        phase_start_both(PhaseKind::Regular, 0.0, 720.0, 1800.0, 2520.0),
        video_play(home, 730.0), // phase 内 → matchClock 10
        video_play(away, 100.0), // phase 外 → 除外
    ];
    let p = ScoreProgressionProjection::build(&make_video_match(home, away), &facts).unwrap();
    assert_eq!(p.points.len(), 4); // goal は 1 件のみ
    assert_eq!(p.points.last().map(|pt| pt.diff()), Some(-1)); // home goal のみ反映
}
