//! 検証結果の集約と表示。
//!
//! `issue` は境界ワイヤ形式 `{scope, code, params}`（ADR 0002）に統一する。
//! domain validation はコアの serde 出力そのまま、decode / 突合系は CLI 所有の
//! scope（`io` / `json` / `sampleDecode` / `corpus`）で同形に揃える。

use serde::Serialize;
use serde_json::{Value, json};

use handball_toolkit::sample_dto::SampleMatchDecodeErrorV2;

/// 指摘の発生段階。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// ファイル読込失敗。
    Read,
    /// JSON / DTO の decode 失敗。
    Decode,
    /// DTO → domain 変換の失敗（`SampleMatchDecodeErrorV2`）。
    Convert,
    /// validators による domain validation。
    Domain,
    /// index ↔ ファイルの突合・転記整合（コーパス検証）。
    Corpus,
}

/// 1 件の検証指摘。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// 検証対象ファイル（呼び出し時のパス表記のまま）。
    pub path: String,
    pub stage: Stage,
    /// fact 単位の指摘のときの `facts[]` index（JSON 配列順）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fact_index: Option<usize>,
    /// fact 単位の指摘のときのコーパス factID。
    #[serde(rename = "factID", skip_serializing_if = "Option::is_none")]
    pub fact_id: Option<String>,
    /// 構造化エラー本体（`{scope, code, params}`）。
    pub issue: Value,
}

impl Finding {
    pub fn new(path: &str, stage: Stage, issue: Value) -> Finding {
        Finding {
            path: path.to_owned(),
            stage,
            fact_index: None,
            fact_id: None,
            issue,
        }
    }

    /// 人間可読の 1 行表示。文言レイヤは持たず code + params を機械的に並べる。
    pub fn human_line(&self) -> String {
        let scope = self
            .issue
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let code = self
            .issue
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let mut line = format!("{}: [{scope}/{code}]", self.path);
        if let Some(params) = self.issue.get("params")
            && params.as_object().is_none_or(|map| !map.is_empty())
        {
            line.push(' ');
            line.push_str(&params.to_string());
        }
        if let Some(index) = self.fact_index {
            line.push_str(&format!(" (facts[{index}]"));
            if let Some(id) = &self.fact_id {
                line.push_str(&format!(" factID={id}"));
            }
            line.push(')');
        }
        line
    }
}

/// 実行全体の集計。`--json` 出力の形そのもの。
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub checked_files: usize,
    pub findings: Vec<Finding>,
}

impl RunReport {
    pub fn finding_count(&self) -> usize {
        self.findings.len()
    }
}

/// `SampleMatchDecodeErrorV2` はコア側で Serialize を持たない（FFI 専用型）ため、
/// ワイヤ形式への写像は CLI が所有する。code は variant 名の camelCase。
pub fn sample_decode_issue(error: &SampleMatchDecodeErrorV2) -> Value {
    use SampleMatchDecodeErrorV2 as E;
    let (code, params) = match error {
        E::SchemaVersionMismatch { found, expected } => (
            "schemaVersionMismatch",
            json!({"found": found, "expected": expected}),
        ),
        E::UnknownConfigurationKind(value) => ("unknownConfigurationKind", json!({"value": value})),
        E::MissingConfigurationPayload(value) => {
            ("missingConfigurationPayload", json!({"value": value}))
        }
        E::UnknownPayloadKind(value) => ("unknownPayloadKind", json!({"value": value})),
        E::MissingPayloadBody(value) => ("missingPayloadBody", json!({"value": value})),
        E::UnknownPlayKind(value) => ("unknownPlayKind", json!({"value": value})),
        E::UnknownControlKind(value) => ("unknownControlKind", json!({"value": value})),
        E::UnknownStoppageKind(value) => ("unknownStoppageKind", json!({"value": value})),
        E::UnknownPhaseKind(value) => ("unknownPhaseKind", json!({"value": value})),
        E::UnknownAnchorKind(value) => ("unknownAnchorKind", json!({"value": value})),
        E::UnknownVideoProvider(value) => ("unknownVideoProvider", json!({"value": value})),
        E::MissingAnchorBody(value) => ("missingAnchorBody", json!({"value": value})),
        E::UnknownTeamKey(value) => ("unknownTeamKey", json!({"value": value})),
        E::UnknownPlayerKey(value) => ("unknownPlayerKey", json!({"value": value})),
        E::MissingPhaseStartEnd => ("missingPhaseStartEnd", json!({})),
    };
    json!({"scope": "sampleDecode", "code": code, "params": params})
}

pub fn io_issue(message: &str) -> Value {
    json!({"scope": "io", "code": "readFailed", "params": {"message": message}})
}

pub fn json_issue(message: &str) -> Value {
    json!({"scope": "json", "code": "decodeFailed", "params": {"message": message}})
}

pub fn corpus_issue(code: &str, params: Value) -> Value {
    json!({"scope": "corpus", "code": code, "params": params})
}
