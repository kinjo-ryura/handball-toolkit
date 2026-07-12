//! sample_dto の serde 表現の検証（Rust 新設 — パリティ分子には数えない）。
//!
//! Swift は Codable 合成で JSON 対応を得ておりデコードの単体テストが存在しないが、
//! Rust では明示 rename（`factID` / `externalID`）・明示 null 耐性・RFC 3339 日時が
//! 手書き属性なので、`SAMPLE_DTO_V2.md` の JSON 例と実配信コーパスの断片で固定する。

use chrono::{DateTime, Utc};
use handball_toolkit::sample_dto::{
    SampleFactDtoV2, SampleMatchConfigurationDtoV2, SampleMatchDtoV2, SampleVideoSourceDtoV2,
};
use uuid::Uuid;

// ── configuration tagged union（SAMPLE_DTO_V2.md の JSON 例） ──

#[test]
fn decodes_timer_configuration_snippet() {
    let json = r#"{"kind": "timer", "timer": {"phaseDurationSeconds": 1800}}"#;
    let dto: SampleMatchConfigurationDtoV2 = serde_json::from_str(json).unwrap();
    assert_eq!(dto.kind, "timer");
    assert_eq!(dto.timer.as_ref().unwrap().phase_duration_seconds, 1800.0);
    // 欠落フィールドは None（明示 null と等価に扱う）
    assert!(dto.video.is_none());
    assert!(dto.video_highlight.is_none());
}

#[test]
fn decodes_video_configuration_snippet_with_external_id_spelling() {
    let json =
        r#"{"kind": "video", "video": {"source": {"provider": "youtube", "externalID": "abc"}}}"#;
    let dto: SampleMatchConfigurationDtoV2 = serde_json::from_str(json).unwrap();
    let source = &dto.video.as_ref().unwrap().source;
    assert_eq!(source.provider, "youtube");
    assert_eq!(source.external_id, "abc");
}

#[test]
fn decodes_video_highlight_configuration_snippet() {
    let json = r#"{"kind": "videoHighlight", "videoHighlight": {"source": {"provider": "youtube", "externalID": "abc"}}}"#;
    let dto: SampleMatchConfigurationDtoV2 = serde_json::from_str(json).unwrap();
    assert_eq!(dto.kind, "videoHighlight");
    assert_eq!(
        dto.video_highlight.as_ref().unwrap().source.external_id,
        "abc"
    );
    assert!(dto.video.is_none());
}

// ── facts（実配信コーパス v2/matches/ の断片。明示 null が並ぶ形式） ──

#[test]
fn decodes_corpus_style_play_fact_with_explicit_nulls() {
    let json = r#"{
      "factID": "3cd6eba6-dc74-5823-8d61-73af52f0d35d",
      "payload": {
        "control": null,
        "kind": "play",
        "play": {
          "anchor": {
            "endMatchElapsedSeconds": null,
            "endVideoElapsedSeconds": null,
            "kind": "videoClock",
            "matchClock": null,
            "videoClock": { "elapsedSeconds": 1130.0 }
          },
          "kind": "goal",
          "note": null,
          "playerKey": "E858A482-227F-405D-A42F-700AF84F75F8",
          "relatedPlayerKey": null,
          "teamKey": "home",
          "title": null
        }
      },
      "recordedAt": "2025-12-20T00:00:48Z"
    }"#;
    let dto: SampleFactDtoV2 = serde_json::from_str(json).unwrap();
    assert_eq!(
        dto.fact_id,
        Some(Uuid::parse_str("3cd6eba6-dc74-5823-8d61-73af52f0d35d").unwrap())
    );
    assert_eq!(
        dto.recorded_at,
        "2025-12-20T00:00:48Z".parse::<DateTime<Utc>>().unwrap()
    );
    assert_eq!(dto.payload.kind, "play");
    assert!(dto.payload.control.is_none());
    let play = dto.payload.play.as_ref().unwrap();
    assert_eq!(play.kind, "goal");
    assert_eq!(play.team_key.as_deref(), Some("home"));
    assert_eq!(
        play.player_key.as_deref(),
        Some("E858A482-227F-405D-A42F-700AF84F75F8")
    );
    assert!(play.related_player_key.is_none());
    assert_eq!(play.anchor.kind, "videoClock");
    assert_eq!(
        play.anchor.video_clock.as_ref().unwrap().elapsed_seconds,
        1130.0
    );
    assert!(play.anchor.match_clock.is_none());
    assert!(play.anchor.end_match_elapsed_seconds.is_none());
}

#[test]
fn decodes_phase_start_control_fact_with_omitted_optionals() {
    // 明示 null ではなくキー欠落でも Option フィールドは None になる（factID / stoppage / end）。
    let json = r#"{
      "recordedAt": "2026-01-01T00:00:00Z",
      "payload": {
        "kind": "control",
        "control": {
          "kind": "phaseStart",
          "phaseStart": { "kind": "regular" },
          "anchor": {
            "kind": "matchClock",
            "matchClock": { "elapsedSeconds": 0.0 },
            "endMatchElapsedSeconds": 1800.0
          }
        }
      }
    }"#;
    let dto: SampleFactDtoV2 = serde_json::from_str(json).unwrap();
    assert!(dto.fact_id.is_none());
    assert!(dto.payload.play.is_none());
    let control = dto.payload.control.as_ref().unwrap();
    assert_eq!(control.kind, "phaseStart");
    assert_eq!(control.phase_start.as_ref().unwrap().kind, "regular");
    assert!(control.stoppage.is_none());
    assert_eq!(
        control.anchor.match_clock.as_ref().unwrap().elapsed_seconds,
        0.0
    );
    assert_eq!(control.anchor.end_match_elapsed_seconds, Some(1800.0));
    assert!(control.anchor.end_video_elapsed_seconds.is_none());
}

// ── match body ──

#[test]
fn decodes_minimal_match_body() {
    let json = r#"{
      "schemaVersion": 2,
      "match": {
        "displayName": null,
        "date": "2025-12-21T12:00:00Z",
        "configuration": {"kind": "timer", "timer": {"phaseDurationSeconds": 1800}}
      },
      "teams": {
        "home": {"key": "home", "name": "ホーム", "players": [{"key": "h1", "name": "選手1", "jerseyNumber": 7}]},
        "away": {"key": "away", "name": "アウェイ", "players": [{"key": "a1", "name": "選手2", "jerseyNumber": null}]}
      },
      "facts": []
    }"#;
    let dto: SampleMatchDtoV2 = serde_json::from_str(json).unwrap();
    assert_eq!(dto.schema_version, 2);
    assert!(dto.r#match.display_name.is_none());
    assert_eq!(dto.r#match.configuration.kind, "timer");
    assert_eq!(dto.teams.home.key, "home");
    assert_eq!(dto.teams.home.players[0].jersey_number, Some(7));
    assert!(dto.teams.away.players[0].jersey_number.is_none());
    assert!(dto.facts.is_empty());
}

// ── 明示 rename の表記固定（Swift 表記 externalID / factID を保存） ──

#[test]
fn serializes_renamed_fields_verbatim() {
    let source = SampleVideoSourceDtoV2 {
        provider: "youtube".to_owned(),
        external_id: "abc".to_owned(),
    };
    let value = serde_json::to_value(&source).unwrap();
    assert!(value.get("externalID").is_some());
    assert!(value.get("externalId").is_none());

    let fact_json = r#"{
      "factID": "3cd6eba6-dc74-5823-8d61-73af52f0d35d",
      "recordedAt": "2025-12-20T00:00:48Z",
      "payload": {"kind": "play", "play": null, "control": null}
    }"#;
    let fact: SampleFactDtoV2 = serde_json::from_str(fact_json).unwrap();
    let value = serde_json::to_value(&fact).unwrap();
    assert!(value.get("factID").is_some());
    assert!(value.get("factId").is_none());
}
