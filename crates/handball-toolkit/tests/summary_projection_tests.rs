//! 移植元: `Tests/RecorderDomainTests/SummaryProjectionTests.swift`。
//!
//! （Swift の suite static `homeID` / `awayID` / `alice` / `bob` / `carol` は、
//! Rust では各テスト内のローカル乱数 ID。ID の一貫性は 1 テスト内で閉じる。）

mod fixtures;

use fixtures::{make_timer_match, phase_start_match, play_at_match, play_fact, video_play_fact};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::entities::Match;
use handball_toolkit::facts::{MatchFact, PlayEventKind};
use handball_toolkit::projection::{SummaryProjection, TeamSummaryLine, TimelineProjection};
use uuid::Uuid;

fn summary_with_phases(match_: &Match, facts: &[MatchFact]) -> SummaryProjection {
    SummaryProjection::build_with_timeline(match_, &TimelineProjection::build(match_, facts))
}

#[test]
fn empty_match_produces_zero_summary() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let summary = SummaryProjection::build(&make_timer_match(home, away), &[]);
    assert_eq!(summary.home_score, 0);
    assert_eq!(summary.away_score, 0);
    assert_eq!(summary.home_team.shot_attempts(), 0);
    assert!(summary.player_stats.is_empty());
}

#[test]
fn goals_and_misses_aggregate() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let (alice, bob, carol) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let facts = vec![
        play_fact(PlayEventKind::Goal, home, alice, None),
        play_fact(PlayEventKind::Goal, home, alice, None),
        play_fact(PlayEventKind::ShotMissed, home, alice, None),
        play_fact(PlayEventKind::Goal, away, bob, None),
        play_fact(PlayEventKind::ShotMissed, away, carol, None),
    ];
    let summary = SummaryProjection::build(&make_timer_match(home, away), &facts);
    assert_eq!(summary.home_score, 2);
    assert_eq!(summary.away_score, 1);
    assert_eq!(summary.home_team.goals, 2);
    assert_eq!(summary.home_team.shot_misses, 1);
    assert_eq!(summary.away_team.goals, 1);
    assert_eq!(summary.away_team.shot_misses, 1);

    let alice_line = summary
        .player_stats
        .iter()
        .find(|s| s.player_id == alice)
        .unwrap();
    assert_eq!(alice_line.goals, 2);
    assert_eq!(alice_line.shot_misses, 1);

    let bob_line = summary
        .player_stats
        .iter()
        .find(|s| s.player_id == bob)
        .unwrap();
    assert_eq!(bob_line.goals, 1);
    assert_eq!(bob_line.shot_misses, 0);
}

#[test]
fn cards_and_free_note_are_not_counted_as_shots() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let alice = Uuid::new_v4();
    let facts = vec![
        play_fact(PlayEventKind::Goal, home, alice, None),
        play_fact(PlayEventKind::YellowCard, home, alice, None),
        play_fact(PlayEventKind::TwoMinuteSuspension, home, alice, None),
        play_fact(PlayEventKind::FreeNote, home, alice, Some("メモ")),
    ];
    let summary = SummaryProjection::build(&make_timer_match(home, away), &facts);
    assert_eq!(summary.home_team.goals, 1);
    assert_eq!(summary.home_team.shot_misses, 0);
    assert_eq!(summary.home_team.shot_attempts(), 1);
}

#[test]
fn team_line_scoring_rate() {
    let line = TeamSummaryLine {
        team_id: Uuid::new_v4(),
        goals: 3,
        shot_misses: 2,
    };
    let rate = line.scoring_rate();
    assert!(rate.is_some());
    assert!((rate.unwrap_or(0.0) - 0.6).abs() < 1e-9);
}

#[test]
fn team_line_scoring_rate_is_none_for_zero_attempts() {
    let line = TeamSummaryLine {
        team_id: Uuid::new_v4(),
        goals: 0,
        shot_misses: 0,
    };
    assert_eq!(line.scoring_rate(), None);
}

// ── timer/video 対称性 ──

/// SummaryProjection は anchor を見ない (kind/teamID/playerID のみ集計) ため、
/// 同じイベントを matchClock anchor (timer) と videoClock anchor (video) で記録しても
/// 同一の集計を返す。timer→video 移行で stats がズレないことを保証する。
#[test]
fn summary_is_anchor_agnostic_timer_vs_video_symmetry() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let (alice, bob, carol) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let events = [
        (PlayEventKind::Goal, home, alice),
        (PlayEventKind::Goal, away, bob),
        (PlayEventKind::ShotMissed, home, alice),
        (PlayEventKind::ShotMissed, away, carol),
    ];
    let timer_facts: Vec<MatchFact> = events
        .iter()
        .map(|&(kind, team, player)| play_fact(kind, team, player, None))
        .collect();
    let video_facts: Vec<MatchFact> = events
        .iter()
        .map(|&(kind, team, player)| video_play_fact(kind, team, player))
        .collect();
    let timer_summary = SummaryProjection::build(&make_timer_match(home, away), &timer_facts);
    let video_summary = SummaryProjection::build(&make_timer_match(home, away), &video_facts);
    assert_eq!(timer_summary, video_summary);
}

// ── 集計の分岐 (playerID なし / 未知 team / redCard / sort) ──

/// playerID 無しの goal は team スコアに載るが player 行は作られない。
#[test]
fn goal_without_player_counts_team_but_no_player_line() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let mut fact = play_fact(PlayEventKind::Goal, home, Uuid::new_v4(), None);
    if let handball_toolkit::facts::MatchFactPayload::Play(play) = &mut fact.payload {
        play.player_id = None;
    }
    let summary = SummaryProjection::build(&make_timer_match(home, away), &[fact]);
    assert_eq!(summary.home_score, 1);
    assert!(summary.player_stats.is_empty());
}

/// home/away どちらでもない teamID の goal はチームスコアに載らない (が player 集計は team 非依存)。
#[test]
fn goal_with_unknown_team_counts_neither_side() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let alice = Uuid::new_v4();
    let facts = vec![play_fact(PlayEventKind::Goal, Uuid::new_v4(), alice, None)];
    let summary = SummaryProjection::build(&make_timer_match(home, away), &facts);
    assert_eq!(summary.home_score, 0);
    assert_eq!(summary.away_score, 0);
    assert_eq!(
        summary
            .player_stats
            .iter()
            .find(|s| s.player_id == alice)
            .map(|s| s.goals),
        Some(1)
    );
}

/// redCard は shot にも player 集計にも載らない。
#[test]
fn red_card_is_not_counted_as_shot_or_player_stat() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let facts = vec![play_fact(
        PlayEventKind::RedCard,
        home,
        Uuid::new_v4(),
        None,
    )];
    let summary = SummaryProjection::build(&make_timer_match(home, away), &facts);
    assert_eq!(summary.home_team.shot_attempts(), 0);
    assert!(summary.player_stats.is_empty());
}

/// playerStats は playerID.uuidString の昇順で安定ソートされる。
#[test]
fn player_stats_sorted_by_uuid_string() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let p1 = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let p2 = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
    let p3 = Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap();
    let facts = vec![
        play_fact(PlayEventKind::Goal, home, p3, None),
        play_fact(PlayEventKind::Goal, home, p1, None),
        play_fact(PlayEventKind::Goal, home, p2, None),
    ];
    let summary = SummaryProjection::build(&make_timer_match(home, away), &facts);
    let ids: Vec<Uuid> = summary.player_stats.iter().map(|s| s.player_id).collect();
    assert_eq!(ids, vec![p1, p2, p3]);
}

// ── phaseSummaries (build_with_timeline) ──

/// build (facts 版) は resolver 非依存なので phase_summaries は常に空 (header は集計される)。
#[test]
fn facts_build_leaves_phase_summaries_empty() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let alice = Uuid::new_v4();
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0),
        play_at_match(PlayEventKind::Goal, home, alice, 100.0),
    ];
    let summary = SummaryProjection::build(&make_timer_match(home, away), &facts);
    assert!(summary.phase_summaries.is_empty());
    assert_eq!(summary.home_score, 1);
}

/// 2 phase の goal / shotMissed が phase 別に集計される。
#[test]
fn phase_summaries_aggregate_per_phase() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
    let match_ = make_timer_match(home, away);
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0),
        phase_start_match(PhaseKind::Regular, 1800.0, 3600.0),
        play_at_match(PlayEventKind::Goal, home, alice, 100.0),
        play_at_match(PlayEventKind::Goal, away, bob, 200.0),
        play_at_match(PlayEventKind::ShotMissed, home, alice, 300.0),
        play_at_match(PlayEventKind::Goal, home, alice, 2000.0),
    ];
    let summary = summary_with_phases(&match_, &facts);
    assert_eq!(summary.phase_summaries.len(), 2);
    let p1 = &summary.phase_summaries[0];
    assert_eq!(p1.kind, PhaseKind::Regular);
    assert_eq!(p1.regular_index, Some(0));
    assert_eq!(p1.home_goals, 1);
    assert_eq!(p1.away_goals, 1);
    assert_eq!(p1.home_shot_misses, 1);
    assert_eq!(p1.home_attempts(), 2);
    let p2 = &summary.phase_summaries[1];
    assert_eq!(p2.regular_index, Some(1));
    assert_eq!(p2.home_goals, 1);
    assert_eq!(p2.away_goals, 0);
}

/// 記録のない regular phase は省かれるが、後続 phase の regularIndex は詰めずに進む。
#[test]
fn phase_without_records_omitted_but_index_advances() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let alice = Uuid::new_v4();
    let match_ = make_timer_match(home, away);
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0), // 記録なし
        phase_start_match(PhaseKind::Regular, 1800.0, 3600.0), // 後半に goal
        play_at_match(PlayEventKind::Goal, home, alice, 2000.0),
    ];
    let summary = summary_with_phases(&match_, &facts);
    assert_eq!(summary.phase_summaries.len(), 1);
    assert_eq!(summary.phase_summaries[0].regular_index, Some(1)); // 前半が省かれても index は 1
}

/// matchClock を解決できない goal (phase 区間外) は phase 別から除外され、header >= Σphase。
#[test]
fn unresolved_goal_excluded_from_phases_but_counted_in_header() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let alice = Uuid::new_v4();
    let match_ = make_timer_match(home, away);
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0),
        play_at_match(PlayEventKind::Goal, home, alice, 100.0), // phase 内
        play_at_match(PlayEventKind::Goal, home, alice, 5000.0), // どの phase 外
    ];
    let summary = summary_with_phases(&match_, &facts);
    assert_eq!(summary.home_score, 2); // header は両方数える
    let phase_goals: i64 = summary.phase_summaries.iter().map(|p| p.home_goals).sum();
    assert_eq!(phase_goals, 1); // phase 別は 1 件のみ (header >= Σphase)
}

/// shootout goal は shootout 行に集計され regularIndex は None。
#[test]
fn shootout_goals_tally_into_shootout_line_with_none_index() {
    let (home, away) = (Uuid::new_v4(), Uuid::new_v4());
    let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
    let match_ = make_timer_match(home, away);
    let facts = vec![
        phase_start_match(PhaseKind::Regular, 0.0, 1800.0),
        phase_start_match(PhaseKind::Shootout, 1800.0, 1800.0),
        play_at_match(PlayEventKind::Goal, home, alice, 100.0),
        play_at_match(PlayEventKind::Goal, away, bob, 1800.0),
    ];
    let summary = summary_with_phases(&match_, &facts);
    assert_eq!(summary.phase_summaries.len(), 2);
    let shootout = summary
        .phase_summaries
        .iter()
        .find(|p| p.kind == PhaseKind::Shootout)
        .unwrap();
    assert_eq!(shootout.regular_index, None);
    assert_eq!(shootout.away_goals, 1);
}
