//! ポゼッション区間 projection（handball-project#217 / #220）。Rust 新設のためオラクル（Swift）は無い。
//!
//! 語の定義は HandballRecorder の `CONTEXT.md`「ポゼッション (Possession)」。
//! **区間の終わりは `明示 end → 区間内の同チーム goal → 次のポゼッション開始 / phase end` の順で
//! 決めて必ずクランプする**、**数える単位は fact の件数ではなくチームが切り替わった回数**、の
//! 2 つがこの projection の全部で、どちらも「置かないルール」（同一チームの連続を許す /
//! 欠測を拒否しない / 順序の逆転を validation で弾かない）とセットで成立している。

mod fixtures;

use fixtures::{epoch, make_video_match, phase_start_both, video_play};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::facts::{
    MatchFact, MatchFactPayload, PlayEventKind, PlayFact, PossessionFact,
};
use handball_toolkit::ids::{FactId, PlayerId, TeamId};
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
            end_anchor: None,
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
            end_anchor: None,
        }),
    }
}

/// videoClock の始まりと明示 end を持つポゼッション（handball-project#220）。
fn possession_at_video_with_end(
    team_id: TeamId,
    video_secs: f64,
    end_video_secs: f64,
) -> MatchFact {
    let MatchFact {
        id,
        recorded_at,
        payload: MatchFactPayload::Possession(possession),
    } = possession_at_video(team_id, video_secs)
    else {
        unreachable!("possession_at_video が possession を返さない");
    };
    MatchFact {
        id,
        recorded_at,
        payload: MatchFactPayload::Possession(PossessionFact {
            end_anchor: Some(FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end_video_secs,
            })),
            ..possession
        }),
    }
}

/// videoClock に置いた `shotMissed`（goal ではないので区間を閉じない側の確認用）。
fn shot_missed_at_video(team_id: TeamId, video_secs: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::new_v4()),
        recorded_at: epoch(),
        payload: MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::ShotMissed,
            team_id: Some(team_id),
            player_id: Some(PlayerId(Uuid::new_v4())),
            related_player_id: None,
            anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: video_secs,
            }),
            title: None,
            note: None,
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

// ── 終わりの決め方（handball-project#220）──
//
// 優先順は `明示 end → 区間内の同チーム goal → 次の開始 / phase end` で、**どれも上界
// （次の開始 / phase end）でクランプする**。ここが崩れると区間が重なり、攻撃長の分布
// （#148 が総体チェックに使う）が静かに膨らむ／歪む。

/// 明示 end が書かれていれば、次のポゼッション開始より手前でそこで閉じる。
/// **死球時間が直前のポゼッションに入らなくなる**のがこの変更の目的。
#[test]
fn explicit_end_closes_the_segment_before_the_next_possession() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video_with_end(home, 200.0, 230.0), // matchClock 100 → 130
        possession_at_video(away, 260.0),                 // matchClock 160
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(p.segments[0].match_elapsed_end, 130.0);
    assert_eq!(
        p.segments[0].video_elapsed_end,
        Some(230.0),
        "video も明示 end 側から採る"
    );
    assert_eq!(
        p.segments[0].match_elapsed_duration(),
        30.0,
        "導出 end（60 秒）ではなく明示 end の 30 秒"
    );
}

/// 明示 end が上界（次のポゼッション開始）より後ろでも**エラーにせずクランプする**。
/// 供給源は機械で 112〜202 件 / 試合を書き、順序の逆転は故障ではなく常態。
#[test]
fn explicit_end_past_the_next_possession_is_clamped_not_rejected() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video_with_end(home, 200.0, 400.0), // 次の開始（260）より後ろ
        possession_at_video(away, 260.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(p.segments.len(), 2, "拒否せず区間は作る");
    assert!(p.unresolved_fact_ids.is_empty());
    assert_eq!(p.segments[0].match_elapsed_end, 160.0, "上界でクランプ");
    assert_eq!(
        p.segments[0].video_elapsed_end,
        Some(260.0),
        "クランプが効いたら video も上界側から採る（明示 end の 400 は使わない）"
    );
    assert!(
        p.segments[0].match_elapsed_end <= p.segments[1].match_elapsed_start,
        "区間の重なりは構造的に起こりえない"
    );
}

/// phase の end より後ろの明示 end も同じくクランプする。
#[test]
fn explicit_end_past_the_phase_end_is_clamped() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video_with_end(home, 1800.0, 2500.0), // phase end は video 1900
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(p.segments[0].match_elapsed_end, 1800.0);
    assert_eq!(p.segments[0].video_elapsed_end, Some(1900.0));
}

/// 明示 end が無ければ、**区間内の同チーム goal** が次の上界になる。
/// goal は「今使っている上界（次の開始）より締まった上界」。
#[test]
fn same_team_goal_inside_the_span_tightens_the_end() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0), // matchClock 100
        video_play(home, 240.0),          // home の goal（matchClock 140）
        possession_at_video(away, 260.0), // matchClock 160
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(p.segments[0].match_elapsed_end, 140.0, "goal で閉じる");
    assert_eq!(p.segments[0].video_elapsed_end, Some(240.0));
    assert_eq!(
        p.segments[1].match_elapsed_start, 160.0,
        "次の区間の始まりは動かない（隙間 = ボールデッド）"
    );
}

/// **相手チームの goal では閉じない。** 被得点の瞬間はこのポゼッションの終わりではあるが、
/// goal を上界に採る根拠は「得点したチームの攻撃がそこで終わった」— 相手の goal は
/// このポゼッションの区間内に入らない（入るなら取りこぼしの側）。
#[test]
fn opponent_goal_does_not_close_the_segment() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0),
        video_play(away, 240.0), // away の goal
        possession_at_video(away, 260.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);

    assert_eq!(
        p.segments[0].match_elapsed_end, 160.0,
        "次のポゼッション開始まで伸びる（従来どおり）"
    );
}

/// `shotMissed` では閉じない（リバウンドで同じチームの攻撃が続きうる）。
#[test]
fn shot_missed_does_not_close_the_segment() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0),
        shot_missed_at_video(home, 240.0),
        possession_at_video(away, 260.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(p.segments[0].match_elapsed_end, 160.0);
}

/// 区間内に同チーム goal が 2 件あれば**最初の 1 件**で閉じる（一番締まった上界）。
/// 2 件入るのは後続のポゼッション開始を取りこぼしたときで、この区間を実際に終わらせたのは
/// 1 件目の得点。
#[test]
fn first_goal_wins_when_several_fall_inside_the_span() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0),
        video_play(home, 240.0), // matchClock 140
        video_play(home, 250.0), // matchClock 150（取りこぼした次の攻撃の得点）
        possession_at_video(away, 300.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(p.segments[0].match_elapsed_end, 140.0);
}

/// **明示 end が goal より優先される。** 供給源が終わりを出せたなら、それが一番良い値。
#[test]
fn explicit_end_wins_over_a_goal_in_the_span() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video_with_end(home, 200.0, 220.0), // matchClock 100 → 120
        video_play(home, 240.0),                          // matchClock 140
        possession_at_video(away, 300.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(p.segments[0].match_elapsed_end, 120.0);
}

/// 始まりちょうどの goal は採らない（0 長の区間を作らないため）。次のポゼッション開始で閉じる。
#[test]
fn goal_exactly_at_the_start_is_not_used() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0),
        video_play(home, 200.0), // 同時刻
        possession_at_video(away, 260.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(p.segments[0].match_elapsed_end, 160.0);
    assert!(p.segments[0].match_elapsed_duration() > 0.0);
}

/// 区間の外（次のポゼッション開始より後ろ）の goal は無関係。
#[test]
fn goal_after_the_span_is_ignored() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0),
        possession_at_video(away, 260.0),
        video_play(home, 300.0), // 次の区間の中にある home の goal
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    assert_eq!(p.segments[0].match_elapsed_end, 160.0);
}

/// 明示 end も goal も無ければ、**従来どおり**次の開始 / phase end で閉じる
/// （既存データと、終わりを出せない供給源の出力がそのまま通ること）。
#[test]
fn without_end_or_goal_the_derivation_is_unchanged() {
    let (home, away) = teams();
    let facts = vec![
        first_half(),
        possession_at_video(home, 200.0),
        possession_at_video(away, 260.0),
    ];
    let p = PossessionProjection::build(&make_video_match(home, away), &facts);
    let spans: Vec<(f64, f64)> = p
        .segments
        .iter()
        .map(|s| (s.match_elapsed_start, s.match_elapsed_end))
        .collect();
    assert_eq!(spans, vec![(100.0, 160.0), (160.0, 1800.0)]);
}
