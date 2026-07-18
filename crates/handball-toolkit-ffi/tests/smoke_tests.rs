//! FFI 公開関数のスモークテスト（FFI を越える前に Rust 内で挙動を固定する）。
//! 公開面はコア crate の `ffi_api`（feature `uniffi`）に集約されており、
//! この crate の再エクスポート越しに疎通を確認する。
//! ロジックの正しさはコア側 140 テスト + golden パリティの守備範囲（ここでは見ない）。

use handball_toolkit_ffi::ffi_api;

use handball_toolkit::configuration::MatchConfiguration;
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::projection::SegmentResolver;
use uuid::Uuid;

fn timer_match() -> Match {
    Match {
        id: Uuid::from_u128(1),
        title: Some("スモーク".to_string()),
        date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
        home_team_id: Uuid::from_u128(2),
        away_team_id: Uuid::from_u128(3),
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    }
}

#[test]
fn バージョン文字列を返す() {
    assert_eq!(ffi_api::toolkit_version(), "0.1.0");
}

#[test]
fn 空_log_の_summary_は_0_対_0() {
    let summary = ffi_api::build_summary(timer_match(), vec![]);
    assert_eq!(summary.home_score, 0);
    assert_eq!(summary.away_score, 0);
    assert!(summary.phase_summaries.is_empty());
}

#[test]
fn 空_log_の_timeline_と_validate_delete_が疎通する() {
    let timeline = ffi_api::build_timeline(timer_match(), vec![]);
    assert!(timeline.resolved_facts.is_empty());

    let issues = ffi_api::validate_delete(Uuid::from_u128(9), vec![], timer_match());
    assert!(issues.is_empty());
}

#[test]
fn segment_resolver_の_ffi_コンストラクタが疎通する() {
    let resolver = SegmentResolver::build_from_facts(vec![]);
    assert!(resolver.all_segments().is_empty());
    assert!(resolver.all_phases().is_empty());
    assert_eq!(resolver.phase_kind_ffi(0.0), None);
}
