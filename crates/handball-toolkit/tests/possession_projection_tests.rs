//! ポゼッション区間 projection（handball-project#217）。Rust 新設のためオラクル（Swift）は無い。
//!
//! 語の定義は HandballRecorder の `CONTEXT.md`「ポゼッション (Possession)」。
//! **区間の終わりは次のポゼッション開始、無ければその phase の end**、**数える単位は fact の
//! 件数ではなくチームが切り替わった回数**、の 2 つがこの projection の全部で、どちらも
//! 「置かないルール」（同一チームの連続を許す / 欠測を拒否しない）とセットで成立している。

mod fixtures;

use fixtures::{epoch, make_video_match, phase_start_both};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::facts::{MatchFact, MatchFactPayload, PossessionFact};
use handball_toolkit::ids::{FactId, TeamId};
use handball_toolkit::projection::PossessionProjection;
use uuid::Uuid;

/// videoClock に置いたポゼッション開始。CV 出力（動画解析）が出すのはこの形。
fn possession_at_video(team_id: TeamId, video_secs: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::new_v4()),
        recorded_at: epoch(),
        payload: MatchFactPayload::Possession(PossessionFact {
            team_id,
            anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: video_secs,
            }),
        }),
    }
}

/// matchClock だけに置いたポゼッション開始（動画に紐付かない fact の確認用）。
fn possession_at_match(team_id: TeamId, match_secs: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::new_v4()),
        recorded_at: epoch(),
        payload: MatchFactPayload::Possession(PossessionFact {
            team_id,
            anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: match_secs,
            }),
        }),
    }
}

fn teams() -> (TeamId, TeamId) {
    (TeamId(Uuid::new_v4()), TeamId(Uuid::new_v4()))
}

/// 1H = matchClock 0..1800 / videoClock 100..1900（同尺・オフセット 100 秒）。
fn first_half() -> MatchFact {
    phase_start_both(PhaseKind::Regular, 0.0, 100.0, 1800.0, 1900.0)
}

// ── 区間の切り出し ──

/// 終わりは**次のポゼッション開始**。最後の 1 件だけが phase の end で閉じる。
#[test]
fn segment_ends_at_next_possession_and_last_at_phase_end() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0), // matchClock 100
        possession_at_video(away, 260.0), // matchClock 160
        possession_at_video(home, 300.0), // matchClock 200
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(p.segments.len(), 3);
    assert!(p.unresolved_fact_ids.is_empty());

    let spans: Vec<(f64, f64)> = p
        .segments
        .iter()
        .map(|s| (s.match_elapsed_start, s.match_elapsed_end))
        .collect();
    assert_eq!(spans, vec![(100.0, 160.0), (160.0, 200.0), (200.0, 1800.0)]);

    let video: Vec<(Option<f64>, Option<f64>)> = p
        .segments
        .iter()
        .map(|s| (s.video_elapsed_start, s.video_elapsed_end))
        .collect();
    assert_eq!(
        video,
        vec![
            (Some(200.0), Some(260.0)),
            (Some(260.0), Some(300.0)),
            (Some(300.0), Some(1900.0)),
        ]
    );

    let durations: Vec<f64> = p
        .segments
        .iter()
        .map(|s| s.match_elapsed_duration())
        .collect();
    assert_eq!(durations, vec![60.0, 40.0, 1600.0]);
}

/// 出現順が時刻順とずれていても matchClock 昇順に並べ直す。
#[test]
fn segments_are_sorted_by_match_clock() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 300.0),
        possession_at_video(away, 200.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    let starts: Vec<f64> = p.segments.iter().map(|s| s.match_elapsed_start).collect();
    assert_eq!(starts, vec![100.0, 200.0]);
    assert_eq!(p.segments[0].team_id, away);
    assert_eq!(p.segments[1].team_id, home);
}

/// ポゼッションが 1 件も無い試合は空の projection（None ではない — 区間 0 は正常な状態）。
#[test]
fn no_possession_facts_yields_empty_projection() {
    let (home, away) = teams();
    let p = PossessionProjection::build(&make_video_match(home, away), &[first_half()]);
    assert!(p.segments.is_empty());
    assert_eq!(p.possession_count, 0);
    assert!(p.unresolved_fact_ids.is_empty());
}

// ── phase をまたがない ──

/// 1H 最後の区間は **1H の end** で閉じる（2H の 1 件目まで伸びない）。
#[test]
fn segment_does_not_span_phases() {
    let (home, away) = teams();
    let second_half = phase_start_both(PhaseKind::Regular, 1800.0, 2000.0, 3600.0, 3800.0);
    let facts = vec![
        first_half(),
        second_half,
        possession_at_video(home, 1800.0), // 1H の matchClock 1700
        possession_at_video(away, 2100.0), // 2H の matchClock 1900
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(p.segments.len(), 2);
    assert_eq!(
        p.segments[0].match_elapsed_end, 1800.0,
        "1H の end で閉じる"
    );
    assert_eq!(p.segments[0].video_elapsed_end, Some(1900.0));
    assert_eq!(p.segments[1].match_elapsed_start, 1900.0);
    assert_eq!(p.segments[1].match_elapsed_end, 3600.0);
    assert_ne!(p.segments[0].phase_fact_id, p.segments[1].phase_fact_id);
}

// ── 冗長宣言と数え方 ──

/// 同じチームが連続したら 2 件目は冗長。**区間は消さず**、ポゼッション数だけ数えない。
#[test]
fn repeated_team_marks_redundant_without_dropping_the_segment() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0),
        possession_at_video(home, 260.0), // 冗長な宣言
        possession_at_video(away, 300.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(
        p.segments.len(),
        3,
        "冗長でも区間は残す（fact を編集できる必要がある）"
    );
    let redundant: Vec<bool> = p.segments.iter().map(|s| s.is_redundant).collect();
    assert_eq!(redundant, vec![false, true, false]);
    assert_eq!(
        p.possession_count, 2,
        "数える単位はチームが切り替わった回数"
    );
}

/// phase をまたいだ同一チームは冗長ではない（新しい phase の 1 件目は必ず数える）。
#[test]
fn same_team_across_phases_is_not_redundant() {
    let (home, away) = teams();
    let second_half = phase_start_both(PhaseKind::Regular, 1800.0, 2000.0, 3600.0, 3800.0);
    let facts = vec![
        first_half(),
        second_half,
        possession_at_video(home, 1800.0), // 1H
        possession_at_video(home, 2100.0), // 2H
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(p.segments.len(), 2);
    assert!(!p.segments[1].is_redundant);
    assert_eq!(p.possession_count, 2);
}

// ── 区間にできない fact ──

/// phase の外（phase 開始前）に置かれた fact は区間にできない。**黙って捨てず**に報告する。
#[test]
fn possession_outside_any_phase_is_reported_as_unresolved() {
    let (home, away) = teams();
    let outside = possession_at_video(home, 50.0); // 1H は video 100 から
    let inside = possession_at_video(away, 200.0);
    let outside_id = outside.id;
    let facts = vec![first_half(), outside, inside];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(p.segments.len(), 1);
    assert_eq!(p.unresolved_fact_ids, vec![outside_id]);
    assert_eq!(p.possession_count, 1);
}

/// phase が 1 つも無ければ全件が unresolved（区間の終わりを定義できない）。
#[test]
fn no_phase_makes_every_possession_unresolved() {
    let (home, away) = teams();
    let facts = vec![
        possession_at_video(home, 200.0),
        possession_at_video(away, 300.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    assert!(p.segments.is_empty());
    assert_eq!(p.unresolved_fact_ids.len(), 2);
}

/// matchClock だけに置いた fact も区間になる。video を解決できれば video も埋まる
/// （動画モードの phase は両時計を持つので resolver が引ける）。
#[test]
fn match_clock_anchored_possession_still_builds_a_segment() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_match(home, 100.0),
        possession_at_match(away, 160.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(p.segments.len(), 2);
    assert_eq!(p.segments[0].match_elapsed_start, 100.0);
    assert_eq!(p.segments[0].match_elapsed_end, 160.0);
    assert_eq!(p.segments[0].video_elapsed_start, Some(200.0));
    assert_eq!(p.segments[0].video_elapsed_end, Some(260.0));
}

// ── 参照 ──

/// `segment(fact_id)` で選択中の fact から区間を引ける。
#[test]
fn segment_lookup_by_fact_id() {
    let (home, away) = teams();
    let target = possession_at_video(home, 200.0);
    let target_id = target.id;
    let facts = vec![first_half(), target, possession_at_video(away, 260.0)];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    let found = p
        .segment(target_id)
        .expect("記録した fact の区間が引けない");
    assert_eq!(found.team_id, home);
    assert_eq!(found.match_elapsed_start, 100.0);
    assert!(p.segment(FactId(Uuid::new_v4())).is_none());
}
