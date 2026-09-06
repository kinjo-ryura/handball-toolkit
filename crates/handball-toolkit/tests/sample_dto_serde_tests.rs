//! sample_dto の serde 表現の検証（Rust 新設 — パリティ分子には数えない）。
//!
//! Swift は Codable 合成で JSON 対応を得ておりデコードの単体テストが存在しないが、
//! Rust では明示 rename（`factID` / `externalID`）・明示 null 耐性・RFC 3339 日時が
//! 手書き属性なので、`SAMPLE_DTO_V2.md` の JSON 例と実配信コーパスの断片で固定する。

use chrono::{DateTime, Utc};
use handball_toolkit::sample_dto::{
    SampleFactDtoV2, SampleGeneratorDtoV2, SampleMatchConfigurationDtoV2, SampleMatchDtoV2,
    SampleVideoSourceDtoV2, encode_sample_match,
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
    // 1.6.0 が書いた試合ファイル / 配信サンプルには generator が無い（handball-project#300）
    assert!(dto.generator.is_none());
}

// ── 試合ファイルの optional 拡張（generator / cloudIdentifier / durationSeconds。handball-project#300） ──

/// 1.6.1 が書く試合ファイルの形。`schemaVersion` は 2 のまま（optional の追加は版を上げない —
/// Recorder ADR 0002 規律 1）。
const MATCH_FILE_WITH_EXTRAS: &str = r#"{
  "schemaVersion": 2,
  "generator": {"name": "HandballRecorder", "version": "1.6.1", "build": "29"},
  "match": {
    "date": "2026-09-05T03:00:00Z",
    "configuration": {
      "kind": "video",
      "video": {
        "source": {
          "provider": "local",
          "externalID": "7A0B4C1D-2E3F-4A5B-8C9D-0E1F2A3B4C5D/L0/001",
          "cloudIdentifier": "AwAAAAAAAAAA/mock-cloud-identifier",
          "durationSeconds": 3612.5
        }
      }
    }
  },
  "teams": {
    "home": {"key": "home", "name": "ホーム", "players": []},
    "away": {"key": "away", "name": "アウェイ", "players": []}
  },
  "facts": []
}"#;

#[test]
fn decodes_match_file_extras() {
    let dto: SampleMatchDtoV2 = serde_json::from_str(MATCH_FILE_WITH_EXTRAS).unwrap();
    assert_eq!(dto.schema_version, 2);
    assert_eq!(
        dto.generator,
        Some(SampleGeneratorDtoV2 {
            name: "HandballRecorder".to_owned(),
            version: "1.6.1".to_owned(),
            build: Some("29".to_owned()),
        })
    );
    let source = &dto.r#match.configuration.video.as_ref().unwrap().source;
    assert_eq!(source.provider, "local");
    assert_eq!(
        source.cloud_identifier.as_deref(),
        Some("AwAAAAAAAAAA/mock-cloud-identifier")
    );
    assert_eq!(source.duration_seconds, Some(3612.5));
}

#[test]
fn decodes_generator_without_build() {
    let json = r#"{"name": "handball-video-analysis", "version": "0.3.0"}"#;
    let generator: SampleGeneratorDtoV2 = serde_json::from_str(json).unwrap();
    assert_eq!(generator.name, "handball-video-analysis");
    assert!(generator.build.is_none());
}

/// 拡張はすべて optional で、None のときはキーごと省かれる — 1.6.0 以前のアプリが書いた
/// ファイルと同じバイト列に戻る（`golden/export/` のバイト一致がそのまま成り立つ）。
#[test]
fn serializes_match_file_extras_verbatim_and_omits_when_none() {
    let source = SampleVideoSourceDtoV2 {
        provider: "local".to_owned(),
        external_id: "abc".to_owned(),
        cloud_identifier: Some("cloud".to_owned()),
        duration_seconds: Some(120.0),
    };
    let value = serde_json::to_value(&source).unwrap();
    assert_eq!(value["cloudIdentifier"], "cloud");
    assert_eq!(value["durationSeconds"], 120.0);
    assert!(value.get("cloud_identifier").is_none());

    let none = SampleVideoSourceDtoV2 {
        cloud_identifier: None,
        duration_seconds: None,
        ..source
    };
    let value = serde_json::to_value(&none).unwrap();
    assert!(value.get("cloudIdentifier").is_none());
    assert!(value.get("durationSeconds").is_none());

    let generator = SampleGeneratorDtoV2 {
        name: "HandballRecorder".to_owned(),
        version: "1.6.1".to_owned(),
        build: None,
    };
    let value = serde_json::to_value(&generator).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"name": "HandballRecorder", "version": "1.6.1"})
    );
}

/// `encode_sample_match` はキーをバイト順に並べる（Swift `.sortedKeys` 互換）ので、
/// `generator` は `facts` と `match` の間に入る。試合ファイルの実バイト列を固定する。
#[test]
fn encodes_generator_between_facts_and_match() {
    let dto: SampleMatchDtoV2 = serde_json::from_str(MATCH_FILE_WITH_EXTRAS).unwrap();
    let text = encode_sample_match(&dto);
    let facts = text.find("\"facts\" : ").unwrap();
    let generator = text.find("\"generator\" : {").unwrap();
    let match_ = text.find("\"match\" : {").unwrap();
    assert!(facts < generator && generator < match_, "{text}");
    assert!(text.contains("\"build\" : \"29\""));
    assert!(text.contains("\"cloudIdentifier\" : \"AwAAAAAAAAAA/mock-cloud-identifier\""));
    assert!(text.contains("\"durationSeconds\" : 3612.5"));
    // 再 parse で往復する
    let back: SampleMatchDtoV2 = serde_json::from_str(&text).unwrap();
    assert_eq!(back, dto);
}

// ── 明示 rename の表記固定（Swift 表記 externalID / factID を保存） ──

#[test]
fn serializes_renamed_fields_verbatim() {
    let source = SampleVideoSourceDtoV2 {
        provider: "youtube".to_owned(),
        external_id: "abc".to_owned(),
        cloud_identifier: None,
        duration_seconds: None,
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
