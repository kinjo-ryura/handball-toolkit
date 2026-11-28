//! 日時の書き出しの形を固定する（handball-project#838）。
//!
//! 日時 crate を chrono から jiff へ乗り換えたときに、運ぶ形（同期の payload・wasm へ渡す JSON）と
//! 配信 JSON（SAMPLE_DTO_V2）の日時の書き方が変わらないことを見る。期待値は乗り換え前の chrono の
//! 出力で、このテストは乗り換え前の chrono で緑を確かめてから乗り換えた。
//!
//! - コアの型（`Match.date` / `MatchFact.recordedAt` / `SyncStamp`）: RFC 3339・`Z`・小数秒は
//!   0 / 3 / 6 / 9 桁のうち値を失わない最短（`.1234` は `.123400`）
//! - SAMPLE_DTO_V2: 秒まで・`Z`・秒未満は切り捨て（Swift `.iso8601` 互換）
//!
//! 値は JSON から読んで書き直して比べる（日時 crate の API に依存させないため）。

mod fixtures;

use fixtures::{make_video_match, video_play};
use handball_toolkit::entities::Match;
use handball_toolkit::facts::MatchFact;
use handball_toolkit::ids::TeamId;
use handball_toolkit::sample_dto::SampleMatchDtoV2;
use handball_toolkit::sync::SyncStamp;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

/// （読む文字列, コアの型が書き出す文字列）。
const CORE_CASES: &[(&str, &str)] = &[
    ("2023-11-14T22:13:20Z", "2023-11-14T22:13:20Z"),
    ("2023-11-14T22:13:20.000Z", "2023-11-14T22:13:20Z"),
    ("2023-11-14T22:13:20.123Z", "2023-11-14T22:13:20.123Z"),
    ("2023-11-14T22:13:20.1Z", "2023-11-14T22:13:20.100Z"),
    ("2023-11-14T22:13:20.1234Z", "2023-11-14T22:13:20.123400Z"),
    ("2023-11-14T22:13:20.123456Z", "2023-11-14T22:13:20.123456Z"),
    (
        "2023-11-14T22:13:20.1234567Z",
        "2023-11-14T22:13:20.123456700Z",
    ),
    (
        "2023-11-14T22:13:20.000000001Z",
        "2023-11-14T22:13:20.000000001Z",
    ),
    // 1970 年より前（Unix 秒が負）でも小数秒は正の向きで数える。
    ("1969-12-31T23:59:59.5Z", "1969-12-31T23:59:59.500Z"),
    // オフセット付きは UTC に直して `Z` で書く。
    ("2026-01-01T09:00:00+09:00", "2026-01-01T00:00:00Z"),
    ("2026-01-01T09:00:00.25+09:00", "2026-01-01T00:00:00.250Z"),
];

/// `value` の `field` を `input` に差し替えて `T` として読み、書き直した `field` を返す。
fn rewrite<T: Serialize + DeserializeOwned>(value: &Value, field: &str, input: &str) -> Value {
    let mut value = value.clone();
    value[field] = json!(input);
    let parsed: T = serde_json::from_value(value).expect("日時を差し替えた JSON は読める");
    serde_json::to_value(&parsed).expect("書き出せる")[field].clone()
}

fn sample_match() -> Value {
    let home = TeamId(Uuid::new_v4());
    let away = TeamId(Uuid::new_v4());
    serde_json::to_value(make_video_match(home, away)).unwrap()
}

#[test]
fn match_date_keeps_rfc3339_with_grouped_fraction() {
    let base = sample_match();
    for (input, expected) in CORE_CASES {
        assert_eq!(
            rewrite::<Match>(&base, "date", input),
            json!(expected),
            "{input}"
        );
    }
}

#[test]
fn match_fact_recorded_at_keeps_rfc3339_with_grouped_fraction() {
    let base = serde_json::to_value(video_play(TeamId(Uuid::new_v4()), 1.0)).unwrap();
    for (input, expected) in CORE_CASES {
        assert_eq!(
            rewrite::<MatchFact>(&base, "recordedAt", input),
            json!(expected),
            "{input}"
        );
    }
}

#[test]
fn sync_stamp_keeps_rfc3339_with_grouped_fraction() {
    for (input, expected) in CORE_CASES {
        let alive = json!({"updatedAt": input});
        assert_eq!(
            rewrite::<SyncStamp>(&alive, "updatedAt", input),
            json!(expected),
            "{input}"
        );
        let deleted = json!({"updatedAt": input, "deletedAt": input});
        assert_eq!(
            rewrite::<SyncStamp>(&deleted, "deletedAt", input),
            json!(expected),
            "{input}"
        );
    }
}

#[test]
fn sync_stamp_without_deleted_at_writes_null() {
    let stamp: SyncStamp = serde_json::from_value(json!({"updatedAt": "2026-01-01T00:00:00Z"}))
        .expect("deletedAt が無くても読める");
    assert_eq!(
        serde_json::to_value(stamp).unwrap(),
        json!({"updatedAt": "2026-01-01T00:00:00Z", "deletedAt": null})
    );
}

/// SAMPLE_DTO_V2 は秒未満を切り捨てる（`.999` も繰り上げない・負の Unix 秒でも過去へ寄せる）。
#[test]
fn sample_dto_dates_truncate_to_seconds() {
    let cases = [
        ("2023-11-14T22:13:20Z", "2023-11-14T22:13:20Z"),
        ("2023-11-14T22:13:20.999Z", "2023-11-14T22:13:20Z"),
        ("2023-11-14T22:13:20.999999999Z", "2023-11-14T22:13:20Z"),
        ("1969-12-31T23:59:59.5Z", "1969-12-31T23:59:59Z"),
        ("2026-01-01T09:00:00.5+09:00", "2026-01-01T00:00:00Z"),
    ];
    let base: Value = serde_json::from_str(include_str!(
        "golden/inputs/matches/2025-12-20-f352ea46.json"
    ))
    .unwrap();
    for (input, expected) in cases {
        let mut value = base.clone();
        value["match"]["date"] = json!(input);
        value["facts"][0]["recordedAt"] = json!(input);
        let dto: SampleMatchDtoV2 = serde_json::from_value(value).expect("読める");
        let written = serde_json::to_value(&dto).unwrap();
        assert_eq!(written["match"]["date"], json!(expected), "{input}");
        assert_eq!(
            written["facts"][0]["recordedAt"],
            json!(expected),
            "{input}"
        );
    }
}
