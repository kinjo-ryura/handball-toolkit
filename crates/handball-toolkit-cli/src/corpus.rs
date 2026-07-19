//! v2 ルートディレクトリの一括検証。
//!
//! index ↔ ファイルの突合（missing / orphan / slug 重複）と、SCHEMA.md が定義する
//! 転記フィールドの整合（homeScore / awayScore = goal 集計、hasVideo、factCount、
//! homeTeamName / awayTeamName、date）を検証する。index が読めない場合は突合を
//! 諦め、見つかった本体ファイルの個別検証だけ行う（per-file の保証は維持する）。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use handball_toolkit::sample_dto::{
    SampleHighlightIndexDtoV2, SampleHighlightSummaryV2, SampleIndexDtoV2, SampleMatchDtoV2,
    SampleMatchSummaryV2,
};

use crate::report::{Finding, RunReport, Stage, corpus_issue, json_issue};
use crate::validate::{self, DerivedMatchInfo};

pub fn validate_corpus(root: &Path, report: &mut RunReport) {
    validate_match_side(root, report);
    validate_highlight_side(root, report);
}

// ── matches 側 ──

fn validate_match_side(root: &Path, report: &mut RunReport) {
    let index_path = root.join("index.json");
    let matches_dir = root.join("matches");

    let listed = read_index::<SampleIndexDtoV2>(&index_path, report).map(|index| {
        validate::validate_match_index_dto(&index_path.display().to_string(), &index, report);
        index.matches
    });

    let Some(entries) = listed else {
        validate_all_bodies(&matches_dir, &[], report);
        return;
    };

    let mut checked_slugs: BTreeSet<&str> = BTreeSet::new();
    for entry in &entries {
        let file = matches_dir.join(format!("{}.json", entry.slug));
        let label = file.display().to_string();
        if !file.is_file() {
            report.findings.push(Finding::new(
                &label,
                Stage::Corpus,
                corpus_issue("missingMatchFile", json!({"slug": entry.slug})),
            ));
            continue;
        }
        // 重複 slug は index 検証で指摘済み。同一ファイルの二重検証を避ける。
        if !checked_slugs.insert(&entry.slug) {
            continue;
        }
        let Some(dto) = validate::read_match_file(&file, report) else {
            continue;
        };
        let derived = validate::validate_match_dto(&label, &dto, report);
        check_match_consistency(&label, entry, &dto, derived.as_ref(), report);
    }

    let listed_slugs: BTreeSet<&str> = entries.iter().map(|entry| entry.slug.as_str()).collect();
    for file in json_files(&matches_dir) {
        if !listed_slugs.contains(slug_of(&file)) {
            report.findings.push(Finding::new(
                &file.display().to_string(),
                Stage::Corpus,
                corpus_issue("orphanMatchFile", json!({})),
            ));
        }
    }
}

/// index との転記整合（SCHEMA.md: homeScore / awayScore は goal 集計の転記、
/// hasVideo は `.video` / `.videoHighlight` のとき true）。
fn check_match_consistency(
    label: &str,
    entry: &SampleMatchSummaryV2,
    dto: &SampleMatchDtoV2,
    derived: Option<&DerivedMatchInfo>,
    report: &mut RunReport,
) {
    if entry.date != dto.r#match.date {
        report.findings.push(Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "dateMismatch",
                json!({"index": entry.date.to_rfc3339(), "body": dto.r#match.date.to_rfc3339()}),
            ),
        ));
    }
    let Some(derived) = derived else {
        return;
    };
    if entry.home_score != derived.home_score || entry.away_score != derived.away_score {
        report.findings.push(Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "scoreMismatch",
                json!({
                    "indexHome": entry.home_score,
                    "indexAway": entry.away_score,
                    "computedHome": derived.home_score,
                    "computedAway": derived.away_score,
                }),
            ),
        ));
    }
    if entry.has_video != derived.has_video {
        report.findings.push(Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "hasVideoMismatch",
                json!({"index": entry.has_video, "body": derived.has_video}),
            ),
        ));
    }
}

// ── highlights 側 ──

fn validate_highlight_side(root: &Path, report: &mut RunReport) {
    let highlights_dir = root.join("highlights");
    if !highlights_dir.is_dir() {
        // ハイライトを配信していないコーパスも許容する。
        return;
    }
    let index_path = highlights_dir.join("index.json");

    let listed = read_index::<SampleHighlightIndexDtoV2>(&index_path, report).map(|index| {
        validate::validate_highlight_index_dto(&index_path.display().to_string(), &index, report);
        index.highlights
    });

    let Some(entries) = listed else {
        validate_all_bodies(&highlights_dir, &["index.json"], report);
        return;
    };

    let mut checked_slugs: BTreeSet<&str> = BTreeSet::new();
    for entry in &entries {
        let file = highlights_dir.join(format!("{}.json", entry.slug));
        let label = file.display().to_string();
        if !file.is_file() {
            report.findings.push(Finding::new(
                &label,
                Stage::Corpus,
                corpus_issue("missingHighlightFile", json!({"slug": entry.slug})),
            ));
            continue;
        }
        if !checked_slugs.insert(&entry.slug) {
            continue;
        }
        let Some(dto) = validate::read_match_file(&file, report) else {
            continue;
        };
        let derived = validate::validate_match_dto(&label, &dto, report);
        check_highlight_consistency(&label, entry, &dto, derived.as_ref(), report);
    }

    let listed_slugs: BTreeSet<&str> = entries.iter().map(|entry| entry.slug.as_str()).collect();
    for file in json_files(&highlights_dir) {
        if file.file_name().is_some_and(|name| name == "index.json") {
            continue;
        }
        if !listed_slugs.contains(slug_of(&file)) {
            report.findings.push(Finding::new(
                &file.display().to_string(),
                Stage::Corpus,
                corpus_issue("orphanHighlightFile", json!({})),
            ));
        }
    }
}

/// index との転記整合（SCHEMA.md: factCount は本体 facts 配列の長さ、
/// homeTeamName / awayTeamName は本体チーム名）。
fn check_highlight_consistency(
    label: &str,
    entry: &SampleHighlightSummaryV2,
    dto: &SampleMatchDtoV2,
    derived: Option<&DerivedMatchInfo>,
    report: &mut RunReport,
) {
    if entry.date != dto.r#match.date {
        report.findings.push(Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "dateMismatch",
                json!({"index": entry.date.to_rfc3339(), "body": dto.r#match.date.to_rfc3339()}),
            ),
        ));
    }
    let Some(derived) = derived else {
        return;
    };
    if entry.fact_count != derived.fact_count as i64 {
        report.findings.push(Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "factCountMismatch",
                json!({"index": entry.fact_count, "body": derived.fact_count}),
            ),
        ));
    }
    for (side, index_name, body_name) in [
        ("home", &entry.home_team_name, &derived.home_team_name),
        ("away", &entry.away_team_name, &derived.away_team_name),
    ] {
        if index_name != body_name {
            report.findings.push(Finding::new(
                label,
                Stage::Corpus,
                corpus_issue(
                    "teamNameMismatch",
                    json!({"side": side, "index": index_name, "body": body_name}),
                ),
            ));
        }
    }
    if entry.has_video != derived.has_video {
        report.findings.push(Finding::new(
            label,
            Stage::Corpus,
            corpus_issue(
                "hasVideoMismatch",
                json!({"index": entry.has_video, "body": derived.has_video}),
            ),
        ));
    }
}

// ── 共通ヘルパー ──

/// index を読み decode する。ファイル欠落は `missingIndex`、decode 失敗は指摘に積んで None。
fn read_index<T: serde::de::DeserializeOwned>(
    index_path: &Path,
    report: &mut RunReport,
) -> Option<T> {
    let label = index_path.display().to_string();
    if !index_path.is_file() {
        report.findings.push(Finding::new(
            &label,
            Stage::Corpus,
            corpus_issue("missingIndex", json!({})),
        ));
        return None;
    }
    let text = validate::read_file(index_path, report)?;
    match serde_json::from_str::<T>(&text) {
        Ok(index) => Some(index),
        Err(error) => {
            report.findings.push(Finding::new(
                &label,
                Stage::Decode,
                json_issue(&error.to_string()),
            ));
            None
        }
    }
}

/// index が使えないときのフォールバック: 本体ファイルを個別検証だけする。
fn validate_all_bodies(dir: &Path, exclude: &[&str], report: &mut RunReport) {
    for file in json_files(dir) {
        if file
            .file_name()
            .is_some_and(|name| exclude.iter().any(|ex| name == *ex))
        {
            continue;
        }
        if let Some(dto) = validate::read_match_file(&file, report) {
            validate::validate_match_dto(&file.display().to_string(), &dto, report);
        }
    }
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

fn slug_of(path: &Path) -> &str {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
}
