//! wasm バインディング（handball-project#57）。
//!
//! FFI crate（`handball-toolkit-ffi`）と同じ位置づけの**パッケージング層**で、コアには手を
//! 入れない。担うのは 2 つだけ:
//!
//! 1. JS から呼べる粗粒度エントリの公開 — 設計不変条件 4 のとおり「配信 JSON in →
//!    projection out」の同期バッチ 1 往復にする（細かい getter の応酬で境界を跨がない）
//! 2. 境界のマーシャリング（JSON 文字列 ⇄ ドメイン型）とエラーの構造化
//!
//! **ID 生成はシェル（JS）が行う**。設計不変条件 2 によりコアは UUID を生成しないので、
//! `sample_dto::convert` が要求する新規 ID は JS が `crypto.randomUUID()` で事前生成して
//! 渡す — iOS の `convert_sample_match`（ADR 0004 決定 2）と同じ形。この crate 自身も
//! 乱数を引かないため、`getrandom` の wasm バックエンド設定（`--cfg getrandom_backend`）は不要。
//!
//! JS 側の想定フロー:
//!
//! ```js
//! const n = requiredIdCount(json);
//! const ids = Array.from({ length: n }, () => crypto.randomUUID());
//! const view = JSON.parse(buildMatchView(slug, json, ids));
//! ```
//!
//! 公開面の実体は純粋関数（`parse_match` / `build_match_view`）側にあり、`#[wasm_bindgen]`
//! 側はマーシャリングだけの薄いラッパに留める（ロジックを境界に書かない — ADR 0004 決定 1）。
//! これにより host（native）でそのままテストできる。

use handball_toolkit::entities::{Match, Player, Team};
use handball_toolkit::projection::{SummaryProjection, TimelineProjection};
use handball_toolkit::sample_dto::{self, SampleMatchDtoV2};
use serde::Serialize;
use uuid::Uuid;
use wasm_bindgen::prelude::*;

// ── エラー（ADR 0002: コード + パラメータのみ。ユーザー向け文言はシェル所有）──

/// wasm 境界の失敗。JS へは `JsError` の message に**この enum の JSON** を載せて渡す
/// （`{"code":"invalidJson","message":"..."}`）。JS 側は `JSON.parse(err.message)` で
/// コードを取り、表示文言は自分で持つ。
///
/// `Decode` の `detail` は移植エラー型 `SampleMatchDecodeErrorV2` の Debug 表現で、
/// **開発者診断のみ**（ADR 0002 決定 5 と同じ扱い — ユーザーに見せない）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum WasmError {
    /// JSON が SAMPLE_DTO_V2 schema として parse できない。
    InvalidJson { message: String },
    /// DTO → domain の decode 失敗。
    Decode { detail: String },
    /// 事前生成 ID の不足。`required` 個を生成して呼び直す。
    InsufficientNewIds { required: usize, provided: usize },
    /// 渡された ID 文字列が UUID として読めない。
    InvalidUuid { index: usize, value: String },
}

impl From<WasmError> for JsError {
    fn from(error: WasmError) -> Self {
        // WasmError は plain な derive Serialize（String / usize のみ）なので to_string は失敗しない。
        JsError::new(&serde_json::to_string(&error).unwrap_or_else(|_| {
            r#"{"code":"invalidJson","message":"error serialization failed"}"#.to_string()
        }))
    }
}

// ── 出力（境界の view model。コアの型をそのまま束ねるだけで再定義はしない）──

/// デモ / Web シェルが 1 往復で受け取る読み取り用の束。フィールドはコアの型そのままで、
/// 表示整形（ラベル・並び）は JS 側が持つ。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchView {
    pub r#match: Match,
    pub home_team: Team,
    pub away_team: Team,
    pub players: Vec<Player>,
    pub summary: SummaryProjection,
    pub timeline: TimelineProjection,
}

// ── 純粋関数（host でテストする実体）──

/// 配信 JSON（`/v2/matches/{slug}.json` の本体）を DTO へ。
pub fn parse_match(json: &str) -> Result<SampleMatchDtoV2, WasmError> {
    serde_json::from_str(json).map_err(|error| WasmError::InvalidJson {
        message: error.to_string(),
    })
}

/// `build_match_view` へ渡す事前生成 ID の必要数。
pub fn required_id_count(dto: &SampleMatchDtoV2) -> usize {
    sample_dto::required_id_count(dto)
}

/// DTO → domain 変換 + projection 構築を 1 往復で行う。
///
/// `summary` は `build_with_timeline` 経路で作る（timeline の resolver を再利用するので
/// SegmentResolver を二度組まず、phase 別 stats も付く）。
pub fn build_match_view(
    slug: &str,
    dto: &SampleMatchDtoV2,
    new_ids: &[Uuid],
) -> Result<MatchView, WasmError> {
    let required = sample_dto::required_id_count(dto);
    if new_ids.len() < required {
        return Err(WasmError::InsufficientNewIds {
            required,
            provided: new_ids.len(),
        });
    }

    let mut ids = new_ids.iter().copied();
    let converted = sample_dto::convert(slug, dto, None, || {
        // 不足は直前の required_id_count 検査で InsufficientNewIds に落ちている
        // （ADR 0002 決定 6 の表を参照。個数一致は insufficient_ids_boundary テストが担保）。
        ids.next().expect("必要数は事前検査済み")
    })
    .map_err(|error| WasmError::Decode {
        detail: format!("{error:?}"),
    })?;

    let timeline = TimelineProjection::build(&converted.r#match, &converted.facts);
    let summary = SummaryProjection::build_with_timeline(&converted.r#match, &timeline);

    Ok(MatchView {
        r#match: converted.r#match,
        home_team: converted.home_team,
        away_team: converted.away_team,
        players: converted.players,
        summary,
        timeline,
    })
}

/// JS が渡す UUID 文字列列を parse する（境界のマーシャリング）。
pub fn parse_ids(new_ids: &[String]) -> Result<Vec<Uuid>, WasmError> {
    new_ids
        .iter()
        .enumerate()
        .map(|(index, value)| {
            Uuid::parse_str(value).map_err(|_| WasmError::InvalidUuid {
                index,
                value: value.clone(),
            })
        })
        .collect()
}

// ── wasm 境界（マーシャリングのみ。ロジックを書かない）──

/// ツールキットのバージョン文字列。疎通確認の最小関数。
/// workspace で version を共有しているのでコア crate と同値。
#[wasm_bindgen(js_name = toolkitVersion)]
pub fn toolkit_version_js() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// `buildMatchView` へ渡す ID の必要数。JS はこの数だけ `crypto.randomUUID()` を生成する。
#[wasm_bindgen(js_name = requiredIdCount)]
pub fn required_id_count_js(json: &str) -> Result<usize, JsError> {
    let dto = parse_match(json)?;
    Ok(required_id_count(&dto))
}

/// 配信 JSON → `MatchView` の JSON 文字列。JS 側は `JSON.parse` で受ける。
///
/// 戻り値を JsValue ではなく文字列にしているのは、境界を 1 回の serialize に閉じて
/// serde-wasm-bindgen 依存を持たないため（粗粒度バッチ 1 往復 — 設計不変条件 4）。
#[wasm_bindgen(js_name = buildMatchView)]
pub fn build_match_view_js(
    slug: &str,
    json: &str,
    new_ids: Vec<String>,
) -> Result<String, JsError> {
    let dto = parse_match(json)?;
    let ids = parse_ids(&new_ids)?;
    let view = build_match_view(slug, &dto, &ids)?;
    // MatchView は derive Serialize の plain struct なので to_string は失敗しない
    // （ADR 0002 決定 6 の表: sample_match_encoder.rs と同じ根拠）。
    Ok(serde_json::to_string(&view).expect("MatchView は常に serialize 可能"))
}
