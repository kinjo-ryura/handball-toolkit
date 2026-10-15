//! 配信データのルートディレクトリの一括検証。一覧の置き場は 2 つの構成を読む（`match_index_path`）。
//!
//! index ↔ ファイルの突合（missing / orphan / slug 重複）と、SCHEMA.md が定義する
//! 転記フィールドの整合（homeScore / awayScore = goal 集計、hasVideo、factCount、
//! homeTeamName / awayTeamName、date）を検証する。index が読めない場合は突合を
//! 諦め、見つかった本体ファイルの個別検証だけ行う（per-file の保証は維持する）。
//!
//! 大会（`competitions/`。handball-project#566）は `index.json` と本体 `matches/` を
//! 共有するもう 1 組の一覧なので、matches 側の突合に合流させる: 本体は `index.json` か
//! 1 つの大会のどちらか一方にだけ載る（`slugListedTwice`）、どこにも載らない本体は
//! `orphanMatchFile`、大会の一覧の項目はチーム名の転記が必須（`missingTeamName`）。

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use handball_toolkit::sample_dto::{
    SampleCompetitionIndexDtoV2, SampleHighlightIndexDtoV2, SampleHighlightSummaryV2,
    SampleIndexDtoV2, SampleMatchDtoV2, SampleMatchSummaryV2,
};

use crate::report::{Finding, RunReport, Stage, corpus_issue, json_issue};
use crate::validate::{self, DerivedMatchInfo};

pub fn validate_corpus(root: &Path, report: &mut RunReport) {
    validate_match_side(root, report);
    validate_highlight_side(root, report);
}

// ── matches 側 ──

/// 「注目の試合」の一覧の置き場。配信データの構成は 2 つあり、本体と同じ `matches/` の中
/// （`matches/index.json`。3 つの一覧が `matches/` / `highlights/` / `competitions/` に揃う形）と、
/// root 直下（`index.json`。版の名前のディレクトリ `v2/` を root にする形）を読む
/// （handball-project#588）。`matches/index.json` があればそれを、無ければ root 直下を読む。
/// どちらも無いときの `missingIndex` は `matches/index.json` を指す。
fn match_index_path(root: &Path) -> PathBuf {
    let nested = root.join("matches").join("index.json");
    let legacy = root.join("index.json");
    if !nested.is_file() && legacy.is_file() {
        legacy
    } else {
        nested
    }
}

fn validate_match_side(root: &Path, report: &mut RunReport) {
    let index_path = match_index_path(root);
    let matches_dir = root.join("matches");

    let listed = read_index::<SampleIndexDtoV2>(&index_path, report).map(|index| {
        let label = index_path.display().to_string();
        validate::validate_match_index_dto(&label, &index, report);
        // 表（順位表など）は大会の一覧だけが持つ（SCHEMA.md「表」。handball-project#567）。
        if index.tables.is_some() {
            report.findings.push(Finding::new(
                &label,
                Stage::Corpus,
                corpus_issue("unexpectedTables", json!({})),
            ));
        }
        index.matches
    });

    // `matches/index.json` は一覧なので、本体として読まない。
    let Some(entries) = listed else {
        validate_all_bodies(&matches_dir, &["index.json"], report);
        return;
    };

    // 本体がどの一覧に載っているか（slug → 最初に載っていた一覧）。index.json と大会の
    // 一覧をまたいだ重複と、どこにも載らない本体（orphan）の判定に使う。
    let mut listed_in: BTreeMap<String, PathBuf> = BTreeMap::new();
    check_listed_matches(
        &index_path,
        &entries,
        &matches_dir,
        false,
        &mut listed_in,
        report,
    );
    for (list_path, list_entries) in read_competition_lists(root, report) {
        check_listed_matches(
            &list_path,
            &list_entries,
            &matches_dir,
            true,
            &mut listed_in,
            report,
        );
    }

    for file in json_files(&matches_dir) {
        if file.file_name().is_some_and(|name| name == "index.json") {
            continue;
        }
        if !listed_in.contains_key(slug_of(&file)) {
            report.findings.push(Finding::new(
                &file.display().to_string(),
                Stage::Corpus,
                corpus_issue("orphanMatchFile", json!({})),
            ));
        }
    }
}

/// 1 つの一覧（`index.json` か大会の `competitions/{id}.json`）の項目を本体と突き合わせる。
///
/// `requires_team_names` は大会の一覧で true。大会の一覧はアプリが本体を取らずに行を組む
/// （SCHEMA.md「大会」）ので、チーム名の転記が無いと行にチーム名が出ない。
fn check_listed_matches(
    list_path: &Path,
    entries: &[SampleMatchSummaryV2],
    matches_dir: &Path,
    requires_team_names: bool,
    listed_in: &mut BTreeMap<String, PathBuf>,
    report: &mut RunReport,
) {
    let list_label = list_path.display().to_string();
    let mut seen_here: BTreeSet<&str> = BTreeSet::new();
    for entry in entries {
        if !validate::is_safe_slug(&entry.slug) {
            report_unsafe_slug(list_path, &entry.slug, report);
            continue;
        }
        // 同じ一覧内の重複 slug は index 検証（duplicateSlug）で指摘済み。
        // 同一ファイルの二重検証を避ける。
        if !seen_here.insert(&entry.slug) {
            continue;
        }
        match listed_in.entry(entry.slug.clone()) {
            Entry::Occupied(first) => {
                report.findings.push(Finding::new(
                    &list_label,
                    Stage::Corpus,
                    corpus_issue(
                        "slugListedTwice",
                        json!({"slug": entry.slug, "alsoIn": first.get().display().to_string()}),
                    ),
                ));
                continue;
            }
            Entry::Vacant(vacant) => {
                vacant.insert(list_path.to_path_buf());
            }
        }
        if requires_team_names && (entry.home_team_name.is_none() || entry.away_team_name.is_none())
        {
            report.findings.push(Finding::new(
                &list_label,
                Stage::Corpus,
                corpus_issue("missingTeamName", json!({"slug": entry.slug})),
            ));
        }
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
        let Some(dto) = validate::read_match_file(&file, report) else {
            continue;
        };
        let derived = validate::validate_match_dto(&label, &dto, report);
        check_match_consistency(&label, entry, &dto, derived.as_ref(), report);
    }
}

/// `competitions/` を読み、大会ごと（部門を持つ大会は部門ごと）の試合一覧（ファイルパスと項目）を返す。
///
/// ディレクトリが無ければ大会を配信していないコーパスとして空を返す。大会 index が
/// 読めないときは `missingIndex` / decode の指摘を積んだうえで、置いてある一覧ファイルを
/// そのまま使う（本体の突合まで諦めると、大会の本体が全部 orphan に化ける）。
fn read_competition_lists(
    root: &Path,
    report: &mut RunReport,
) -> Vec<(PathBuf, Vec<SampleMatchSummaryV2>)> {
    let dir = root.join("competitions");
    if !dir.is_dir() {
        return Vec::new();
    }
    let index_path = dir.join("index.json");
    let index_label = index_path.display().to_string();

    let mut lists = Vec::new();
    let mut listed_ids: BTreeSet<String> = BTreeSet::new();
    if let Some(index) = read_index::<SampleCompetitionIndexDtoV2>(&index_path, report) {
        validate::validate_competition_index_dto(&index_label, &index, report);
        for competition in &index.competitions {
            // 部門を持つ大会（handball-project#578）の id はファイル名にならないが、アプリが
            // 識別子として持つので字種は部門と揃える。一覧ファイルは部門の id で引く。
            if competition.divisions.is_some() {
                check_competition_id(&index_label, &competition.id, report);
            }
            for id in competition.listing_ids() {
                // id はそのままファイル名になる。slug と同じ字種に限り、`index` は予約する。
                if !check_competition_id(&index_label, id, report) {
                    continue;
                }
                // 重複 id は index 検証（duplicateCompetitionID）で指摘済み。
                if !listed_ids.insert(id.to_string()) {
                    continue;
                }
                let file = dir.join(format!("{id}.json"));
                if !file.is_file() {
                    report.findings.push(Finding::new(
                        &file.display().to_string(),
                        Stage::Corpus,
                        corpus_issue("missingCompetitionFile", json!({"id": id})),
                    ));
                    continue;
                }
                if let Some(list) = read_competition_list(&file, report) {
                    lists.push((file, list));
                }
            }
        }
    }

    for file in json_files(&dir) {
        if file.file_name().is_some_and(|name| name == "index.json") {
            continue;
        }
        if listed_ids.contains(slug_of(&file)) {
            continue;
        }
        report.findings.push(Finding::new(
            &file.display().to_string(),
            Stage::Corpus,
            corpus_issue("orphanCompetitionFile", json!({})),
        ));
        // 大会 index に載っていなくても、本体の突合には使う（上の doc）。
        if let Some(list) = read_competition_list(&file, report) {
            lists.push((file, list));
        }
    }
    lists
}

/// 大会・部門の id が使える形か。使えなければ `unsafeCompetitionID` を積んで false。
fn check_competition_id(label: &str, id: &str, report: &mut RunReport) -> bool {
    if validate::is_safe_competition_id(id) {
        return true;
    }
    report.findings.push(Finding::new(
        label,
        Stage::Corpus,
        corpus_issue("unsafeCompetitionID", json!({"id": id})),
    ));
    false
}

/// 大会の試合一覧は `/v2/index.json` と同じ形（SCHEMA.md）なので、同じ検査を掛ける。
fn read_competition_list(file: &Path, report: &mut RunReport) -> Option<Vec<SampleMatchSummaryV2>> {
    let list = read_index::<SampleIndexDtoV2>(file, report)?;
    validate::validate_match_index_dto(&file.display().to_string(), &list, report);
    Some(list.matches)
}

/// index との転記整合（SCHEMA.md: homeScore / awayScore は goal 集計の転記、
/// hasVideo は `.video` / `.videoHighlight` のとき true、homeTeamName / awayTeamName は
/// 書いてあれば本体チーム名）。
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
    for (side, index_name, body_name) in [
        ("home", &entry.home_team_name, &derived.home_team_name),
        ("away", &entry.away_team_name, &derived.away_team_name),
    ] {
        if let Some(index_name) = index_name
            && index_name != body_name
        {
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
        if !validate::is_safe_slug(&entry.slug) {
            report_unsafe_slug(&index_path, &entry.slug, report);
            continue;
        }
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

/// index 由来の slug を `Path::join` に渡してよいか（handball-project#285）。
///
/// slug は `index.json` から無検証で来る。この CLI は handball-sample-matches の CI が
/// **fork PR の JSON** に対して回すので、slug は攻撃者が書ける入力になる。`..` や `/` を
/// 含む slug をそのまま `join` すると v2 ルートの外を読みに行く（絶対パスなら `join` が
/// 丸ごと差し替える）。実害は小さい（fork PR は read-only トークン・secrets 無し・
/// 読んだ先を JSON として decode するだけ）が、入力をパスに埋める箇所は検証してから使う。
///
/// 規則は配信側（apps-site の `SLUG_PATTERN`）と iOS 側（`IncomingLinkV2.isValidSlug`）と
/// 同じ: 先頭は英数字、以降は英数字と `-`、1〜64 文字。配信中の全 slug がこの形で、
/// SCHEMA.md の「先頭 `{yyyy-MM-dd}`」もこの部分集合。
/// 形式外の slug は index の指摘として出す（ファイル側の指摘にすると、存在しないパスを
/// label に載せることになる）。
fn report_unsafe_slug(index_path: &Path, slug: &str, report: &mut RunReport) {
    report.findings.push(Finding::new(
        &index_path.display().to_string(),
        Stage::Corpus,
        corpus_issue("unsafeSlug", json!({"slug": slug})),
    ));
}
