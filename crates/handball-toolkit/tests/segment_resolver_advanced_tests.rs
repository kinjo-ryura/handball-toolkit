//! 移植元: `Tests/RecorderDomainTests/SegmentResolverAdvancedTests.swift`。
//!
//! SegmentResolver の本実装で網羅すべき高度シナリオ:
//! - 多 phase の matchClock baseline rolling forward
//! - Stoppage による running segment の carve
//! - `Both` 強制 override (forced re-anchor)
//! - shootout phase の degenerate（matchClock 固定、videoClock のみ進行）
//! - timer mode（videoClock 無し）

mod fixtures;

use fixtures::{
    phase_start_both, shootout_phase, timer_phase, timer_stoppage_marker, video_only_phase,
    video_phase, video_stoppage,
};
use handball_toolkit::clock::{MatchClock, VideoClock};
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::facts::StoppageKind;
use handball_toolkit::ids::FactId;
use handball_toolkit::projection::{SegmentResolver, TimeSegmentKind};
use uuid::Uuid;

// ── 多 phase の baseline rolling forward ──

#[test]
fn two_video_only_phases_accumulate_match_clock() {
    // Phase 1: video 720-2520 (= 30 分)
    // Phase 2: video 3000-4800 (= 30 分、ハーフタイム 8 分挟む)
    // videoClock のみ → matchStart は前 phase の end を継承
    let facts = vec![
        video_only_phase(720.0, 2520.0),
        video_only_phase(3000.0, 4800.0),
    ];
    let resolver = SegmentResolver::build(&facts);

    assert_eq!(resolver.phases.len(), 2);
    assert_eq!(resolver.phases[0].match_elapsed_start, Some(0.0));
    assert_eq!(resolver.phases[0].match_elapsed_end, Some(1800.0));
    assert_eq!(resolver.phases[1].match_elapsed_start, Some(1800.0));
    assert_eq!(resolver.phases[1].match_elapsed_end, Some(3600.0));
}

#[test]
fn video_to_match_spans_across_second_phase() {
    // Phase 2 内の videoClock=4000 (phase 2 start から 1000 秒)
    // matchClock = 1800 + 1000 = 2800
    let facts = vec![
        video_only_phase(720.0, 2520.0),
        video_only_phase(3000.0, 4800.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    let mc = resolver.resolve_match_clock(VideoClock {
        elapsed_seconds: 4000.0,
    });
    assert_eq!(mc.map(|c| c.elapsed_seconds), Some(2800.0));
}

#[test]
fn video_between_phases_returns_none() {
    // Phase 1 end=2520、Phase 2 start=3000、video=2700 は phase 間 → None
    let facts = vec![
        video_only_phase(720.0, 2520.0),
        video_only_phase(3000.0, 4800.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    let mc = resolver.resolve_match_clock(VideoClock {
        elapsed_seconds: 2700.0,
    });
    assert_eq!(mc, None);
}

// ── Stoppage carve ──

#[test]
fn stoppage_carves_running_segment_in_video_mode() {
    // Phase: video 0-1800、stoppage video 600-660 (60 秒タイムアウト)
    let phase_id = FactId(Uuid::new_v4());
    let stoppage_id = FactId(Uuid::new_v4());
    let facts = vec![
        video_phase(phase_id, 0.0, 1800.0),
        video_stoppage(stoppage_id, StoppageKind::Timeout, 600.0, 660.0),
    ];
    let resolver = SegmentResolver::build(&facts);

    // segments: running [0,600), stopped [600,660), running [660,1800)
    assert_eq!(resolver.segments.len(), 3);
    assert_eq!(resolver.segments[0].kind, TimeSegmentKind::Running);
    assert_eq!(resolver.segments[0].video_elapsed_start, Some(0.0));
    assert_eq!(resolver.segments[0].video_elapsed_end, Some(600.0));
    assert_eq!(resolver.segments[0].match_elapsed_start, 0.0);
    assert_eq!(resolver.segments[0].match_elapsed_end, Some(600.0));

    assert_eq!(resolver.segments[1].kind, TimeSegmentKind::Stopped);
    assert_eq!(resolver.segments[1].video_elapsed_start, Some(600.0));
    assert_eq!(resolver.segments[1].video_elapsed_end, Some(660.0));
    assert_eq!(resolver.segments[1].match_elapsed_start, 600.0);
    assert_eq!(resolver.segments[1].match_elapsed_end, Some(600.0)); // 動かない
    assert_eq!(
        resolver.segments[1].stoppage_kind,
        Some(StoppageKind::Timeout)
    );

    assert_eq!(resolver.segments[2].kind, TimeSegmentKind::Running);
    assert_eq!(resolver.segments[2].video_elapsed_start, Some(660.0));
    assert_eq!(resolver.segments[2].video_elapsed_end, Some(1800.0));
    assert_eq!(resolver.segments[2].match_elapsed_start, 600.0);
    assert_eq!(resolver.segments[2].match_elapsed_end, Some(1740.0));
}

#[test]
fn phase_end_match_clock_reflects_total_running() {
    // 60 秒 stoppage が 1 つあると phase end の matchClock は (1800-60) = 1740
    let facts = vec![
        video_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        video_stoppage(FactId(Uuid::new_v4()), StoppageKind::Timeout, 600.0, 660.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(
        resolver.phases.first().and_then(|p| p.match_elapsed_end),
        Some(1740.0)
    );
}

#[test]
fn video_inside_stoppage_maps_to_fixed_match_clock() {
    // stoppage 中の video=630 → matchClock = 600 (stopped 区間の固定値)
    let facts = vec![
        video_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        video_stoppage(FactId(Uuid::new_v4()), StoppageKind::Timeout, 600.0, 660.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    let mc = resolver.resolve_match_clock(VideoClock {
        elapsed_seconds: 630.0,
    });
    assert_eq!(mc.map(|c| c.elapsed_seconds), Some(600.0));
}

#[test]
fn multiple_stoppages_in_single_phase_carve_correctly() {
    // Phase video 0-1800、stoppage 1: 300-360 (60s)、stoppage 2: 1000-1060 (60s)
    let facts = vec![
        video_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        video_stoppage(FactId(Uuid::new_v4()), StoppageKind::Timeout, 300.0, 360.0),
        video_stoppage(FactId(Uuid::new_v4()), StoppageKind::Pause, 1000.0, 1060.0),
    ];
    let resolver = SegmentResolver::build(&facts);

    // segments: running [0,300), stopped [300,360), running [360,1000), stopped [1000,1060), running [1060,1800)
    assert_eq!(resolver.segments.len(), 5);
    let last_running = resolver.segments.last().unwrap();
    assert_eq!(last_running.kind, TimeSegmentKind::Running);
    // matchStart = 300 (1st running 結果) + (1000-360 = 640、2nd running 結果) = 940
    assert_eq!(last_running.match_elapsed_start, 940.0);
    assert_eq!(last_running.match_elapsed_end, Some(1680.0)); // 940 + (1800-1060) = 940+740

    // phase end matchClock = 1800 - 60 - 60 = 1680
    assert_eq!(
        resolver.phases.first().and_then(|p| p.match_elapsed_end),
        Some(1680.0)
    );
}

// ── Both override (forced re-anchor) ──

#[test]
fn both_phase_start_overrides_baseline_match_clock() {
    // Phase 1 videoClock 0-1800 (matchStart=0, matchEnd=1800)
    // Phase 2 Both(match: 5000, video: 2400-4200)
    //   → matchStart=5000 (override、ない場合は 1800 だったはず)
    //   → matchEnd=? → endAnchor Both で matchClock 明示なら その値
    let facts = vec![
        video_only_phase(0.0, 1800.0),
        phase_start_both(PhaseKind::Regular, 5000.0, 2400.0, 6800.0, 4200.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(resolver.phases[1].match_elapsed_start, Some(5000.0));
    assert_eq!(resolver.phases[1].match_elapsed_end, Some(6800.0));
}

#[test]
fn both_phase_start_end_explicitly_sets_match_end() {
    // 1 phase の endAnchor が Both で matchClock 明示 → その値が phase end
    let facts = vec![phase_start_both(
        PhaseKind::Regular,
        0.0,
        0.0,
        1800.0,
        1800.0,
    )];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(
        resolver.phases.first().and_then(|p| p.match_elapsed_end),
        Some(1800.0)
    );
}

// ── shootout (degenerate) ──

#[test]
fn shootout_phase_has_degenerate_match_clock() {
    // Shootout は matchClock 固定。phase の matchEnd == matchStart。
    let facts = vec![
        video_only_phase(0.0, 1800.0),    // 前半相当
        video_only_phase(2700.0, 4500.0), // 後半相当
        shootout_phase(5400.0, 6000.0),
    ];
    let resolver = SegmentResolver::build(&facts);

    let shootout = resolver.phases.last().unwrap();
    assert_eq!(shootout.kind, PhaseKind::Shootout);
    // 前半+後半累計が 3600、shootout は固定 → matchStart == matchEnd == 3600
    assert_eq!(shootout.match_elapsed_start, Some(3600.0));
    assert_eq!(shootout.match_elapsed_end, Some(3600.0));
}

#[test]
fn video_in_shootout_always_resolves_to_shootout_start_match_clock() {
    let facts = vec![
        video_only_phase(0.0, 1800.0),
        shootout_phase(2400.0, 3000.0),
    ];
    let resolver = SegmentResolver::build(&facts);

    // shootout 内のどこを見ても matchClock = 1800 (固定)
    let mc1 = resolver.resolve_match_clock(VideoClock {
        elapsed_seconds: 2500.0,
    });
    let mc2 = resolver.resolve_match_clock(VideoClock {
        elapsed_seconds: 2900.0,
    });
    assert_eq!(mc1.map(|c| c.elapsed_seconds), Some(1800.0));
    assert_eq!(mc2.map(|c| c.elapsed_seconds), Some(1800.0));
}

#[test]
fn phase_kind_returns_shootout_at_shootout_match_clock() {
    let facts = vec![
        video_only_phase(0.0, 1800.0),
        shootout_phase(2400.0, 3000.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(resolver.phase_kind(1800.0), Some(PhaseKind::Shootout));
    assert_eq!(resolver.phase_index(1800.0), None); // shootout は None
}

// ── timer mode ──

#[test]
fn timer_mode_phase_produces_single_running_segment_no_video() {
    let phase_id = FactId(Uuid::new_v4());
    let facts = vec![timer_phase(phase_id, 0.0, 1800.0)];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(resolver.segments.len(), 1);
    let seg = resolver.segments.first().unwrap();
    assert_eq!(seg.kind, TimeSegmentKind::Running);
    assert_eq!(seg.video_elapsed_start, None);
    assert_eq!(seg.video_elapsed_end, None);
    assert_eq!(seg.match_elapsed_start, 0.0);
    assert_eq!(seg.match_elapsed_end, Some(1800.0));
}

#[test]
fn timer_mode_stoppage_marker_does_not_carve_segments() {
    // timer mode の stoppage は endAnchor なしの marker。segment は carve しない。
    let facts = vec![
        timer_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        timer_stoppage_marker(FactId(Uuid::new_v4()), StoppageKind::Timeout, 600.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    // timer mode では segment は phase 単一 (stoppage は marker、carve しない)
    assert_eq!(resolver.segments.len(), 1);
}

// ── match→video (resolve_video_clock) と往復一貫性 ──

/// match→video→match の往復一貫性 (stoppage で carve された phase)。
/// running 区間内の matchClock は resolve_video_clock → resolve_match_clock で元に戻る。
/// resolve_video_clock は MigrateToVideoStore が play fact 変換に使う中核なので、ここがズレると
/// 移行した全試合の動画シーク位置が壊れる。
#[test]
fn match_to_video_to_match_round_trip_with_carve() {
    let facts = vec![
        video_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        video_stoppage(FactId(Uuid::new_v4()), StoppageKind::Timeout, 600.0, 660.0),
    ];
    let resolver = SegmentResolver::build(&facts);

    for match_elapsed in [300.0, 1000.0] {
        let video = resolver.resolve_video_clock(MatchClock {
            elapsed_seconds: match_elapsed,
        });
        assert!(video.is_some());
        let back = resolver.resolve_match_clock(video.unwrap());
        assert_eq!(back.map(|c| c.elapsed_seconds), Some(match_elapsed));
    }
    // 具体値の固定: 前半 running はそのまま、後半 running は stoppage 分 (60s) だけ video が先行。
    assert_eq!(
        resolver
            .resolve_video_clock(MatchClock {
                elapsed_seconds: 300.0
            })
            .map(|c| c.elapsed_seconds),
        Some(300.0)
    );
    assert_eq!(
        resolver
            .resolve_video_clock(MatchClock {
                elapsed_seconds: 1000.0
            })
            .map(|c| c.elapsed_seconds),
        Some(1060.0)
    );
}

/// stoppage 開始の matchClock 値 (= 再開後 running の起点 matchClock) は、running 優先で
/// 再開後の video 位置にマップされる (stopped の凍結 video ではなく)。
/// resolve_video_clock の running→stopped 2 段 lookup の優先順位を固定する。
#[test]
fn resolve_video_clock_at_stoppage_boundary_prefers_running_resumption() {
    let facts = vec![
        video_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        video_stoppage(FactId(Uuid::new_v4()), StoppageKind::Timeout, 600.0, 660.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    // matchClock=600 は stoppage 開始時刻だが、running 優先で再開後 video=660 にマップ (600 ではない)。
    assert_eq!(
        resolver
            .resolve_video_clock(MatchClock {
                elapsed_seconds: 600.0
            })
            .map(|c| c.elapsed_seconds),
        Some(660.0)
    );
}

// ── 3 phase の rolling baseline (stoppage 複合) ──

/// 前半 + 後半 + 延長の 3 phase で、各 phase の stoppage が rolling baseline を縮め、
/// 次 phase の matchStart に正しく繰り上がる (累積秒の連続性)。
#[test]
fn three_phases_with_stoppages_accumulate_rolling_baseline() {
    let facts = vec![
        video_phase(FactId(Uuid::new_v4()), 0.0, 1800.0),
        video_stoppage(FactId(Uuid::new_v4()), StoppageKind::Timeout, 600.0, 660.0), // 60s
        video_phase(FactId(Uuid::new_v4()), 2400.0, 4200.0),
        video_stoppage(FactId(Uuid::new_v4()), StoppageKind::Pause, 3000.0, 3060.0), // 60s
        video_phase(FactId(Uuid::new_v4()), 5000.0, 5600.0), // 延長 600s, stoppage なし
    ];
    let resolver = SegmentResolver::build(&facts);

    assert_eq!(resolver.phases.len(), 3);
    assert_eq!(resolver.phases[0].match_elapsed_start, Some(0.0));
    assert_eq!(resolver.phases[0].match_elapsed_end, Some(1740.0)); // 1800 - 60
    assert_eq!(resolver.phases[1].match_elapsed_start, Some(1740.0)); // 前 phase end を継承
    assert_eq!(resolver.phases[1].match_elapsed_end, Some(3480.0)); // 1740 + (1800 - 60)
    assert_eq!(resolver.phases[2].match_elapsed_start, Some(3480.0)); // 継承
    assert_eq!(resolver.phases[2].match_elapsed_end, Some(4080.0)); // 3480 + 600

    // phase2 の stoppage 後 (video=3500) は match=2340 + (3500-3060) = 2780。
    assert_eq!(
        resolver
            .resolve_match_clock(VideoClock {
                elapsed_seconds: 3500.0
            })
            .map(|c| c.elapsed_seconds),
        Some(2780.0)
    );
    // phase3 (延長) の video=5300 は match=3480 + (5300-5000) = 3780。
    assert_eq!(
        resolver
            .resolve_match_clock(VideoClock {
                elapsed_seconds: 5300.0
            })
            .map(|c| c.elapsed_seconds),
        Some(3780.0)
    );
}

// ── 境界値 (半開 [start, end)) ──

/// 単一 video phase の境界値: start は含み、end は排他 (half-open) で両方向とも None 化する。
#[test]
fn resolve_boundary_values_are_half_open() {
    let resolver = SegmentResolver::build(&[video_only_phase(0.0, 1800.0)]);

    // video→match
    assert_eq!(
        resolver
            .resolve_match_clock(VideoClock {
                elapsed_seconds: 0.0
            })
            .map(|c| c.elapsed_seconds),
        Some(0.0)
    );
    assert_eq!(
        resolver
            .resolve_match_clock(VideoClock {
                elapsed_seconds: 1799.0
            })
            .map(|c| c.elapsed_seconds),
        Some(1799.0)
    );
    assert_eq!(
        resolver.resolve_match_clock(VideoClock {
            elapsed_seconds: 1800.0
        }),
        None
    ); // end 排他
    // match→video
    assert_eq!(
        resolver
            .resolve_video_clock(MatchClock {
                elapsed_seconds: 0.0
            })
            .map(|c| c.elapsed_seconds),
        Some(0.0)
    );
    assert_eq!(
        resolver.resolve_video_clock(MatchClock {
            elapsed_seconds: 1800.0
        }),
        None
    ); // matchEnd 排他
}

/// phase 境界ちょうど (前 phase end == 次 phase start の matchClock) は half-open で後の phase に帰属。
#[test]
fn phase_boundary_belongs_to_later_phase() {
    let facts = vec![
        video_only_phase(0.0, 1800.0),
        video_only_phase(2400.0, 4200.0), // matchStart=1800 (rolling), matchEnd=3600
    ];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(resolver.phase_index(1799.0), Some(0));
    assert_eq!(resolver.phase_index(1800.0), Some(1)); // 境界は後の phase
    assert_eq!(resolver.phase_index(0.0), Some(0));
}

// ── shootout の方向非対称性 ──

/// shootout は matchClock が degenerate (start==end) なので、video→match は固定値に解決できる一方、
/// match→video は解決不能 (None)。shootout の play は videoClock anchor で直接記録されるため
/// match→video の逆引きは設計上不要であり、この非対称性を固定する。
#[test]
fn shootout_match_clock_resolves_from_video_but_not_reverse() {
    let facts = vec![
        video_only_phase(0.0, 1800.0),
        shootout_phase(2400.0, 3000.0),
    ];
    let resolver = SegmentResolver::build(&facts);
    // video→match: shootout 内の video は常に固定 matchClock 1800 に解決。
    assert_eq!(
        resolver
            .resolve_match_clock(VideoClock {
                elapsed_seconds: 2700.0
            })
            .map(|c| c.elapsed_seconds),
        Some(1800.0)
    );
    // match→video: degenerate な matchClock 1800 はどの segment にも contain されず None。
    assert_eq!(
        resolver.resolve_video_clock(MatchClock {
            elapsed_seconds: 1800.0
        }),
        None
    );
}

// ── PhaseStart の sort ──

/// PhaseStart fact が時間逆順 (後半を先) に与えられても、primary clock で sort され
/// rolling baseline が正しい順序で繰り上がる。
#[test]
fn phase_starts_given_out_of_order_are_sorted_by_primary_clock() {
    let facts = vec![
        video_only_phase(2400.0, 4200.0), // 後半を先に渡す
        video_only_phase(0.0, 1800.0),    // 前半を後に渡す
    ];
    let resolver = SegmentResolver::build(&facts);
    assert_eq!(resolver.phases[0].video_elapsed_start, Some(0.0)); // sort 後は前半が先頭
    assert_eq!(resolver.phases[1].video_elapsed_start, Some(2400.0));
    assert_eq!(resolver.phases[0].match_elapsed_start, Some(0.0));
    assert_eq!(resolver.phases[1].match_elapsed_start, Some(1800.0)); // 前半 end を継承
}
