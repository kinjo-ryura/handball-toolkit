//! 単一ファイルの検証（試合本体 / 試合 index / ハイライト index）。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use handball_toolkit::configuration::{MatchConfiguration, PhaseKind};
use handball_toolkit::facts::{ControlFact, MatchFact, MatchFactPayload};
use handball_toolkit::persistence_order::persistence_ordered;
use handball_toolkit::projection::SummaryProjection;
use handball_toolkit::sample_dto::{
    SCHEMA_VERSION_CURRENT, SampleFactAnchorDtoV2, SampleHighlightIndexDtoV2, SampleIndexDtoV2,
    SampleMatchDtoV2, convert,
};
use handball_toolkit::validation::DomainValidationIssue;
use handball_toolkit::validators::{
    RosterContext, validate_configuration, validate_fact_log, validate_match, validate_match_fact,
};

use crate::report::{
    Finding, RunReport, Stage, corpus_issue, io_issue, json_issue, sample_decode_issue,
};

/// 試合本体の検証から導出される、index 突合用の値。
pub struct DerivedMatchInfo {
    pub home_score: i64,
    pub away_score: i64,
    pub fact_count: usize,
    /// `.video` / `.videoHighlight` のとき true（SCHEMA.md の `hasVideo` 定義）。
    pub has_video: bool,
    pub home_team_name: String,
    pub away_team_name: String,
}

/// トップレベルキーで形状を自動判別して検証する（単一ファイルモード）。
/// `facts` = 試合本体 / `matches` = 試合 index / `highlights` = ハイライト index。
pub fn validate_file(path: &Path, report: &mut RunReport) {
    let label = path.display().to_string();
    let Some(text) = read_file(path, report) else {
        return;
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            report.findings.push(Finding::new(
                &label,
                Stage::Decode,
                json_issue(&error.to_string()),
            ));
            return;
        }
    };
    let keys = value.as_object();
    if keys.is_some_and(|map| map.contains_key("facts")) {
        if let Some(dto) = decode_match_text(&label, &text, report) {
            validate_match_dto(&label, &dto, report);
        }
    } else if keys.is_some_and(|map| map.contains_key("matches")) {
        match serde_json::from_str::<SampleIndexDtoV2>(&text) {
            Ok(index) => validate_match_index_dto(&label, &index, report),
            Err(error) => report.findings.push(Finding::new(
                &label,
                Stage::Decode,
                json_issue(&error.to_string()),
            )),
        }
    } else if keys.is_some_and(|map| map.contains_key("highlights")) {
        match serde_json::from_str::<SampleHighlightIndexDtoV2>(&text) {
            Ok(index) => validate_highlight_index_dto(&label, &index, report),
            Err(error) => report.findings.push(Finding::new(
                &label,
                Stage::Decode,
                json_issue(&error.to_string()),
            )),
        }
    } else {
        report.findings.push(Finding::new(
            &label,
            Stage::Decode,
            json!({"scope": "json", "code": "unrecognizedShape", "params": {}}),
        ));
    }
}

/// ファイル読込。checked_files のカウントはここで一元化する。
pub fn read_file(path: &Path, report: &mut RunReport) -> Option<String> {
    report.checked_files += 1;
    match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) => {
            report.findings.push(Finding::new(
                &path.display().to_string(),
                Stage::Read,
                io_issue(&error.to_string()),
            ));
            None
        }
    }
}

/// 試合本体ファイルを読み込んで DTO を返す（corpus 側からも使う）。失敗は指摘に積む。
pub fn read_match_file(path: &Path, report: &mut RunReport) -> Option<SampleMatchDtoV2> {
    let label = path.display().to_string();
    let text = read_file(path, report)?;
    decode_match_text(&label, &text, report)
}

fn decode_match_text(label: &str, text: &str, report: &mut RunReport) -> Option<SampleMatchDtoV2> {
    match serde_json::from_str::<SampleMatchDtoV2>(text) {
        Ok(dto) => Some(dto),
        Err(error) => {
            report.findings.push(Finding::new(
                label,
                Stage::Decode,
                json_issue(&error.to_string()),
            ));
            None
        }
    }
}

/// 試合本体 DTO の検証: convert → validate_match / validate_configuration /
/// fact 毎 validate_match_fact / validate_fact_log。convert 失敗時は None。
pub fn validate_match_dto(
    label: &str,
    dto: &SampleMatchDtoV2,
    report: &mut RunReport,
) -> Option<DerivedMatchInfo> {
    // DTO を見れば分かる検査は convert より前に置く。変換に失敗するファイルでも
    // これらの指摘は出したい（convert 失敗時はこの関数が早期 return するため）。
    check_duplicate_fact_ids(label, dto, report);
    check_unexpected_anchor_end(label, dto, report);

    // ID は表示に出ない使い捨てなので決定的な連番で供給する（コアは ID を生成しない）。
    let mut counter: u128 = 0;
    let conversion = match convert(label, dto, None, || {
        counter += 1;
        Uuid::from_u128(counter)
    }) {
        Ok(conversion) => conversion,
        Err(error) => {
            report.findings.push(Finding::new(
                label,
                Stage::Convert,
                sample_decode_issue(&error),
            ));
            return None;
        }
    };

    let match_ = &conversion.r#match;

    for issue in validate_match(match_) {
        report
            .findings
            .push(Finding::new(label, Stage::Domain, domain_issue(&issue)));
    }
    for issue in validate_configuration(&match_.configuration) {
        report
            .findings
            .push(Finding::new(label, Stage::Domain, domain_issue(&issue)));
    }

    let roster = RosterContext {
        home_team_id: conversion.home_team.id,
        away_team_id: conversion.away_team.id,
        player_team_lookup: conversion
            .players
            .iter()
            .map(|player| (player.id, player.team_id))
            .collect(),
        known_player_ids: Some(conversion.players.iter().map(|player| player.id).collect()),
    };
    for (index, fact) in conversion.facts.iter().enumerate() {
        for issue in validate_match_fact(fact, &match_.configuration, &roster) {
            let mut finding = Finding::new(label, Stage::Domain, domain_issue(&issue));
            finding.fact_index = Some(index);
            finding.fact_id = dto
                .facts
                .get(index)
                .and_then(|fact_dto| fact_dto.fact_id)
                .map(|id| id.to_string());
            report.findings.push(finding);
        }
    }

    let ordered = persistence_ordered(&conversion.facts);
    for issue in validate_fact_log(&ordered, match_) {
        report
            .findings
            .push(Finding::new(label, Stage::Domain, domain_issue(&issue)));
    }

    check_match_coverage(label, &conversion.facts, &match_.configuration, report);

    let summary = SummaryProjection::build(match_, &conversion.facts);
    Some(DerivedMatchInfo {
        home_score: summary.home_score,
        away_score: summary.away_score,
        fact_count: dto.facts.len(),
        has_video: matches!(
            match_.configuration,
            MatchConfiguration::Video(_) | MatchConfiguration::VideoHighlight(_)
        ),
        home_team_name: conversion.home_team.name.clone(),
        away_team_name: conversion.away_team.name.clone(),
    })
}

/// `/v2/index.json` の検証（schemaVersion / slug 重複 / date 降順 / slug 先頭日付）。
pub fn validate_match_index_dto(label: &str, index: &SampleIndexDtoV2, report: &mut RunReport) {
    check_schema_version(label, index.schema_version, report);
    check_index_entries(
        label,
        &index
            .matches
            .iter()
            .map(|entry| (entry.slug.as_str(), entry.date))
            .collect::<Vec<_>>(),
        report,
    );
}

/// `/v2/highlights/index.json` の検証（matches 側と同じ配列レベル検査）。
pub fn validate_highlight_index_dto(
    label: &str,
    index: &SampleHighlightIndexDtoV2,
    report: &mut RunReport,
) {
    check_schema_version(label, index.schema_version, report);
    check_index_entries(
        label,
        &index
            .highlights
            .iter()
            .map(|entry| (entry.slug.as_str(), entry.date))
            .collect::<Vec<_>>(),
        report,
    );
}

/// index 配列そのものに掛かる検査。matches / highlights は要素型が違うだけで
/// 不変条件は同じ（SCHEMA.md）ので、`(slug, date)` に落として共有する。
fn check_index_entries(label: &str, entries: &[(&str, DateTime<Utc>)], report: &mut RunReport) {
    check_duplicate_slugs(label, entries.iter().map(|(slug, _)| *slug), report);
    check_date_descending(label, entries, report);
    check_slug_date_prefix(label, entries, report);
}

fn check_schema_version(label: &str, found: i64, report: &mut RunReport) {
    if found != SCHEMA_VERSION_CURRENT {
        report.findings.push(Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "schemaVersionMismatch",
                json!({"found": found, "expected": SCHEMA_VERSION_CURRENT}),
            ),
        ));
    }
}

fn check_duplicate_slugs<'a>(
    label: &str,
    slugs: impl Iterator<Item = &'a str>,
    report: &mut RunReport,
) {
    let mut seen = BTreeSet::new();
    for slug in slugs {
        if !seen.insert(slug) {
            report.findings.push(Finding::new(
                label,
                Stage::Corpus,
                corpus_issue("duplicateSlug", json!({"slug": slug})),
            ));
        }
    }
}

/// index 配列は `date` 降順（SCHEMA.md）。
///
/// アプリは配列順をそのまま表示に使う（`HighlightStoreV2` は index の順序を保持し
/// `MatchListViewV2` はソートしない）ため、ここの退行がそのまま画面の並びの乱れに
/// なる。handball-project#115 で実際に退行し、直った後も再発を止める検査が無かった。
///
/// 同日の試合が複数あるコーパスは普通なので、同値は違反としない（狭義単調ではなく
/// 「新しい順が崩れていない」ことだけを見る）。
fn check_date_descending(label: &str, entries: &[(&str, DateTime<Utc>)], report: &mut RunReport) {
    for pair in entries.windows(2) {
        let (previous_slug, previous_date) = pair[0];
        let (slug, date) = pair[1];
        if date > previous_date {
            report.findings.push(Finding::new(
                label,
                Stage::Corpus,
                corpus_issue(
                    "indexNotDateDescending",
                    json!({
                        "previousSlug": previous_slug,
                        "previousDate": previous_date.to_rfc3339(),
                        "slug": slug,
                        "date": date.to_rfc3339(),
                    }),
                ),
            ));
        }
    }
}

/// slug 先頭 `{yyyy-MM-dd}` は `date` の日付部と一致する（SCHEMA.md の不変条件）。
///
/// `date` は「いつ試合が行われたか」で、記録日時（`recordedAt`）とは独立に動く。
/// slug は人手で付けるので、両者の突合が「`date` に `recordedAt` を流し込む」種の
/// 転記ミス（handball-project#115）を捕まえる。日付部は UTC で比較する — 配信
/// コーパスの `date` は開始時刻不明なら `T00:00:00Z`、分かる場合も日本時間の
/// 日中で、UTC でも同じ日に収まる。
fn check_slug_date_prefix(label: &str, entries: &[(&str, DateTime<Utc>)], report: &mut RunReport) {
    for (slug, date) in entries {
        let expected = date.format("%Y-%m-%d").to_string();
        // 先頭 10 バイト。日付以外の文字が来る / 短すぎる slug は None になり、
        // 「先頭が日付になっていない」も同じ違反として報告する。
        let found = slug.get(..10);
        if found == Some(expected.as_str()) {
            continue;
        }
        report.findings.push(Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "slugDateMismatch",
                json!({"slug": slug, "expected": expected, "found": found}),
            ),
        ));
    }
}

/// 同一試合内で `factID` が重複していないかの検査（index の `duplicateSlug` と同形）。
///
/// `factID` は fact の同一性そのもので、converter は与えられた値をそのまま `FactId`
/// に採用する（`factID` が無い fact にだけシェル注入 ID を採番する）。重複したまま
/// 取り込むと、同じ ID の fact が 2 件できて upsert で潰し合い、記録が黙って消える。
/// コア側の validators は fact 単体しか見ないため、この隙間はどちらからも漏れる。
///
/// `factID` 未設定の fact は採番が衝突しない（`required_id_count` ぶんの新規 UUID を
/// シェルが供給する）ので対象外。
fn check_duplicate_fact_ids(label: &str, dto: &SampleMatchDtoV2, report: &mut RunReport) {
    let mut seen = BTreeSet::new();
    for (index, fact) in dto.facts.iter().enumerate() {
        let Some(id) = fact.fact_id else {
            continue;
        };
        if seen.insert(id) {
            continue;
        }
        let mut finding = Finding::new(
            label,
            Stage::Corpus,
            corpus_issue("duplicateFactID", json!({})),
        );
        finding.fact_index = Some(index);
        finding.fact_id = Some(id.to_string());
        report.findings.push(finding);
    }
}

/// `play` / `possession` の anchor に end 系が入っていないかの検査
/// （SCHEMA.md:「end 系は `phaseStart`（必須）/ `stoppage`（任意）で使用。
/// `play` / `possession` では両方 null」）。
///
/// converter は `play` / `possession` で `decode_end_anchor` を呼ばない。つまり
/// end が書かれていても decode は成功し、**値だけが黙って捨てられる**（逆向きの
/// 「必須なのに無い」は `MissingPhaseStartEnd` で弾かれるので、非対称な隙間）。
/// ポゼッションの供給源（video-analysis, handball-project#178）が誤って区間を
/// 書いた場合にエラーにならず情報が消える経路なので、配信前に blocking で止める。
///
/// 検査はコアではなく CLI に置いた。`SampleMatchDecodeErrorV2` は FFI 境界の
/// uniffi Enum で、variant 追加は ERROR_CODES.md・各シェルの網羅分岐・Android の
/// 文言 2 ロケールへ波及する。供給経路は配信前にこの CLI を必ず通る
/// （sample-matches の CI）ため、error（exit 1）で止めれば risk は塞げる。
fn check_unexpected_anchor_end(label: &str, dto: &SampleMatchDtoV2, report: &mut RunReport) {
    for (index, fact) in dto.facts.iter().enumerate() {
        // どの sub-payload を読むかは converter と同じく `kind` で決める。
        let (payload_kind, anchor): (&str, Option<&SampleFactAnchorDtoV2>) =
            match fact.payload.kind.as_str() {
                "play" => ("play", fact.payload.play.as_ref().map(|play| &play.anchor)),
                "possession" => (
                    "possession",
                    fact.payload
                        .possession
                        .as_ref()
                        .map(|possession| &possession.anchor),
                ),
                _ => continue,
            };
        // sub-payload 欠落は convert が `MissingPayloadBody` で弾く。
        let Some(anchor) = anchor else {
            continue;
        };
        if anchor.end_match_elapsed_seconds.is_none() && anchor.end_video_elapsed_seconds.is_none()
        {
            continue;
        }
        let mut finding = Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "unexpectedAnchorEnd",
                json!({
                    "payloadKind": payload_kind,
                    "endMatchElapsedSeconds": anchor.end_match_elapsed_seconds,
                    "endVideoElapsedSeconds": anchor.end_video_elapsed_seconds,
                }),
            ),
        );
        finding.fact_index = Some(index);
        finding.fact_id = fact.fact_id.map(|id| id.to_string());
        report.findings.push(finding);
    }
}

/// 「記録が試合全体を覆っているか」の検査（handball-project#90）。
///
/// ハンドボールの試合は最低 2 つの regular phase（前半・後半）を持つ。regular な
/// PhaseStart が 2 未満なら前半のみ等の部分記録の疑い（#89 の `2025-12-20-f352ea46`
/// が実例）。本体は単独で内部整合し、index スコアも「前半終了時点の正しい集計」
/// として通るため、ドメイン validation・index 突合のどちらからも漏れる隙間を埋める。
///
/// 単一 phase が正常な `.videoHighlight` は対象外（highlights は PhaseStart を
/// 持たないのが通常。フル試合と区別する内部経路フラグ — SCHEMA.md）。
///
/// 途中で記録をやめた試合を配信サンプルとして残す判断（#89 は displayName /
/// description で明示して残した）を尊重するため、blocking な error ではなく
/// warning で報告する（severity は report.rs の CLI 所有概念）。
fn check_match_coverage(
    label: &str,
    facts: &[MatchFact],
    configuration: &MatchConfiguration,
    report: &mut RunReport,
) {
    if matches!(configuration, MatchConfiguration::VideoHighlight(_)) {
        return;
    }
    let regular_phase_count = facts
        .iter()
        .filter(|fact| {
            matches!(
                &fact.payload,
                MatchFactPayload::Control(ControlFact::PhaseStart(payload))
                    if payload.kind == PhaseKind::Regular
            )
        })
        .count();
    if regular_phase_count < 2 {
        report.findings.push(
            Finding::new(
                label,
                Stage::Corpus,
                corpus_issue(
                    "matchCoverageIncomplete",
                    json!({ "regularPhaseCount": regular_phase_count }),
                ),
            )
            .warning(),
        );
    }
}

fn domain_issue(issue: &DomainValidationIssue) -> Value {
    serde_json::to_value(issue).expect("DomainValidationIssue は常に serialize 可能")
}
