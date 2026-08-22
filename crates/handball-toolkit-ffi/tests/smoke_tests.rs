//! FFI 公開関数のスモークテスト（FFI を越える前に Rust 内で挙動を固定する）。
//! 公開面はコア crate の `ffi_api`（feature `uniffi`）に集約されており、
//! この crate の再エクスポート越しに疎通を確認する。
//! ロジックの正しさはコア側 140 テスト + golden パリティの守備範囲（ここでは見ない）。

use handball_toolkit_ffi::ffi_api;

use handball_toolkit::configuration::MatchConfiguration;
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::ids::{FactId, MatchId, PlayerId, TeamId};
use handball_toolkit::projection::SegmentResolver;
use uuid::Uuid;

fn timer_match() -> Match {
    Match {
        id: MatchId(Uuid::from_u128(1)),
        title: Some("スモーク".to_string()),
        date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
        home_team_id: TeamId(Uuid::from_u128(2)),
        away_team_id: TeamId(Uuid::from_u128(3)),
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    }
}

#[test]
fn バージョン文字列を返す() {
    assert_eq!(ffi_api::toolkit_version(), "0.3.0");
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

    let issues = ffi_api::validate_delete(FactId(Uuid::from_u128(9)), vec![], timer_match());
    assert!(issues.is_empty());
}

#[test]
fn segment_resolver_の_ffi_コンストラクタが疎通する() {
    let resolver = SegmentResolver::build_from_facts(vec![]);
    assert!(resolver.all_segments().is_empty());
    assert!(resolver.all_phases().is_empty());
    assert_eq!(resolver.phase_kind_ffi(0.0), None);
}

#[test]
fn sample_dto_の_parse_convert_export_が疎通する() {
    let json = r#"{
      "schemaVersion": 2,
      "match": {
        "displayName": "スモーク",
        "date": "2026-01-01T00:00:00Z",
        "configuration": {"kind": "timer", "timer": {"phaseDurationSeconds": 1800}}
      },
      "teams": {
        "home": {"key": "home", "name": "Tigers", "players": [{"key": "p1", "name": "Alice"}]},
        "away": {"key": "away", "name": "Falcons", "players": []}
      },
      "facts": []
    }"#;

    let dto = ffi_api::parse_sample_match(json.to_string()).expect("parse 成功");
    let required = ffi_api::sample_match_required_id_count(dto.clone());
    assert_eq!(required, 4); // home team + Alice + away team + match

    // ID 不足は構造化エラー
    let starved = ffi_api::convert_sample_match("smoke".to_string(), dto.clone(), None, vec![]);
    assert_eq!(
        starved.unwrap_err(),
        ffi_api::SampleDtoError::InsufficientNewIds {
            required: 4,
            provided: 0
        }
    );

    let ids: Vec<Uuid> = (1..=required as u128).map(Uuid::from_u128).collect();
    let conversion =
        ffi_api::convert_sample_match("smoke".to_string(), dto, None, ids).expect("convert 成功");
    assert_eq!(conversion.home_team.name, "Tigers");
    assert_eq!(
        conversion.players_by_key["p1"],
        PlayerId(Uuid::from_u128(2))
    );

    // export → encode → 再 parse の round-trip
    let exported = ffi_api::export_sample_match(
        conversion.r#match.clone(),
        conversion.home_team.clone(),
        conversion.away_team.clone(),
        conversion.players.clone(),
        vec![],
        conversion.facts.clone(),
    );
    let encoded = ffi_api::encode_sample_match(exported);
    let reparsed = ffi_api::parse_sample_match(encoded).expect("再 parse 成功");
    assert_eq!(reparsed.r#match.display_name.as_deref(), Some("スモーク"));

    // 不正 JSON は throws（Swift 側）に対応する構造化エラー
    assert!(matches!(
        ffi_api::parse_sample_match("{".to_string()),
        Err(ffi_api::SampleDtoError::InvalidJson { .. })
    ));
}
