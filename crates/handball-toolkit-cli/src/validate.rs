//! 単一ファイルの検証（試合本体 / 試合 index / ハイライト index）。

use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use uuid::Uuid;

use handball_toolkit::configuration::MatchConfiguration;
use handball_toolkit::persistence_order::persistence_ordered;
use handball_toolkit::projection::SummaryProjection;
use handball_toolkit::sample_dto::{
    SCHEMA_VERSION_CURRENT, SampleHighlightIndexDtoV2, SampleIndexDtoV2, SampleMatchDtoV2, convert,
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

/// `/v2/index.json` の検証（schemaVersion / slug 重複）。
pub fn validate_match_index_dto(label: &str, index: &SampleIndexDtoV2, report: &mut RunReport) {
    check_schema_version(label, index.schema_version, report);
    check_duplicate_slugs(
        label,
        index.matches.iter().map(|entry| entry.slug.as_str()),
        report,
    );
}

/// `/v2/highlights/index.json` の検証（schemaVersion / slug 重複）。
pub fn validate_highlight_index_dto(
    label: &str,
    index: &SampleHighlightIndexDtoV2,
    report: &mut RunReport,
) {
    check_schema_version(label, index.schema_version, report);
    check_duplicate_slugs(
        label,
        index.highlights.iter().map(|entry| entry.slug.as_str()),
        report,
    );
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
    let mut seen = std::collections::BTreeSet::new();
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

fn domain_issue(issue: &DomainValidationIssue) -> Value {
    serde_json::to_value(issue).expect("DomainValidationIssue は常に serialize 可能")
}
