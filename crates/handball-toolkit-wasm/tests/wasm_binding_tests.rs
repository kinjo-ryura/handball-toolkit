//! wasm バインディングのテスト。`#[wasm_bindgen]` ラッパはマーシャリングだけなので、
//! 実体である純粋関数を host（native）で回す（wasm ランタイムは要らない）。
//!
//! fixture はコア crate のゴールデン（`export/timer.json`）を借りる。SAMPLE_DTO_V2 の
//! 正典コーパスを二重管理しないため。

use handball_toolkit_wasm::{
    MatchView, WasmError, build_match_view, parse_ids, parse_match, required_id_count,
};

const TIMER_JSON: &str = include_str!("../../handball-toolkit/tests/golden/export/timer.json");

/// 決定的なテスト用 ID（コアも本 crate も UUID を生成しないので、テストが採番する）。
fn ids(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("00000000-0000-4000-8000-{i:012x}"))
        .collect()
}

fn build_from_timer_golden() -> MatchView {
    let dto = parse_match(TIMER_JSON).expect("ゴールデンは SAMPLE_DTO_V2 として parse できる");
    let required = required_id_count(&dto);
    let parsed = parse_ids(&ids(required)).expect("テスト用 ID は妥当な UUID");
    build_match_view("timer", &dto, &parsed).expect("ゴールデンは decode できる")
}

#[test]
fn builds_view_from_delivery_json() {
    let view = build_from_timer_golden();

    assert_eq!(view.home_team.name, "Tigers");
    assert_eq!(view.away_team.name, "Falcons");
    assert_eq!(view.r#match.title.as_deref(), Some("決勝 / ファイナル"));
    // home の goal 1 件 / away は shotMissed 1 件のみ（fixture の play fact 構成）。
    assert_eq!(view.summary.home_score, 1);
    assert_eq!(view.summary.away_score, 0);
    assert_eq!(view.summary.home_team.shot_misses, 0);
    assert_eq!(view.summary.away_team.shot_misses, 1);
    // timeline は fact 全件を保持する（配信 JSON の facts と同数）。
    assert_eq!(view.timeline.resolved_facts.len(), 10);
    // build_with_timeline 経路なので phase 別 stats が付く。
    assert!(!view.summary.phase_summaries.is_empty());
}

#[test]
fn view_serializes_to_camel_case_json() {
    let view = build_from_timer_golden();
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&view).expect("serialize できる"))
            .expect("再 parse できる");

    // JS が読むキー形状の契約（camelCase / `match` は生キー）。
    assert!(json.get("match").is_some());
    assert!(json.get("homeTeam").is_some());
    assert!(json.get("awayTeam").is_some());
    assert_eq!(json["summary"]["homeScore"], 1);
    assert!(json["timeline"]["resolvedFacts"].is_array());
}

/// `build_match_view` の `ids.next().expect` が到達不能である根拠
/// （ADR 0002 決定 6 の表: 数える側と消費する側の一致）。
#[test]
fn insufficient_ids_boundary() {
    let dto = parse_match(TIMER_JSON).expect("parse できる");
    let required = required_id_count(&dto);
    assert!(required > 0, "fixture は ID を消費する");

    // required - 1 個: 消費前に InsufficientNewIds で弾かれる（expect には到達しない）。
    let short = parse_ids(&ids(required - 1)).expect("妥当な UUID");
    assert_eq!(
        build_match_view("timer", &dto, &short).unwrap_err(),
        WasmError::InsufficientNewIds {
            required,
            provided: required - 1,
        }
    );

    // ちょうど required 個: 消費しきって成功する = 数える側と消費する側が一致している。
    let exact = parse_ids(&ids(required)).expect("妥当な UUID");
    assert!(build_match_view("timer", &dto, &exact).is_ok());
}

#[test]
fn reports_invalid_json_as_structured_error() {
    let error = parse_match("{ not json").unwrap_err();
    assert!(matches!(error, WasmError::InvalidJson { .. }));

    // JS が受ける形（message に enum の JSON が載る）。
    let wire: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&error).expect("serialize できる"))
            .expect("再 parse できる");
    assert_eq!(wire["code"], "invalidJson");
}

#[test]
fn reports_invalid_uuid_with_position() {
    let error = parse_ids(&[
        "00000000-0000-4000-8000-000000000000".to_string(),
        "not-a-uuid".to_string(),
    ])
    .unwrap_err();

    assert_eq!(
        error,
        WasmError::InvalidUuid {
            index: 1,
            value: "not-a-uuid".to_string(),
        }
    );
}
