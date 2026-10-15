//! CLI の統合テスト。fixtures のミニコーパス（corpus-ok / corpus-bad）を
//! ライブラリ API と実バイナリの両方で検証する。

use std::path::{Path, PathBuf};
use std::process::Command;

use handball_toolkit_cli::corpus::validate_corpus;
use handball_toolkit_cli::report::{RunReport, Severity};
use handball_toolkit_cli::validate::validate_file;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// 指摘の (code, path 末尾ファイル名) 組を取り出す。
fn finding_codes(report: &RunReport) -> Vec<(String, String)> {
    report
        .findings
        .iter()
        .map(|finding| {
            let code = finding
                .issue
                .get("code")
                .and_then(|value| value.as_str())
                .unwrap_or("?")
                .to_owned();
            let file = Path::new(&finding.path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?")
                .to_owned();
            (code, file)
        })
        .collect()
}

#[test]
fn corpus_ok_has_no_findings() {
    let mut report = RunReport::default();
    validate_corpus(&fixture("corpus-ok"), &mut report);
    assert_eq!(
        report.findings.len(),
        0,
        "指摘なしのはず: {:#?}",
        report.findings
    );
    // index + match 2 件（1 件は大会の一覧にだけ載る）+ highlight index + highlight
    // + 大会 index + 大会の一覧 3 本（部門を持つ大会の 2 部門 + 部門を持たない大会。
    // handball-project#578）= 9 ファイル
    assert_eq!(report.checked_files, 9);
}

/// 大会の一覧（`competitions/{id}.json`）に載った本体は `index.json` に無くても orphan に
/// ならず、index と同じ転記整合（スコア / hasVideo / date / チーム名）を受ける
/// （handball-project#566）。corpus-ok が 0 件なのは上で見ているので、ここでは
/// 「大会の一覧だけに載る本体が突合の対象に入っている」ことを、その本体を壊した
/// 一時コーパスで固定する。その本体は部門の一覧（`test-league-2026-men.json`。
/// handball-project#578）に載っているので、部門の一覧が突合に入っていることの証拠も兼ねる。
#[test]
fn competition_listed_match_is_checked_against_its_body() {
    // dev-dependency を増やさないため、std だけで一時コーパスを作る。
    let dir = std::env::temp_dir().join(format!(
        "handball-toolkit-cli-competition-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&fixture("corpus-ok"), &dir);
    // 大会の一覧の転記（homeScore 1 / awayScore 0）と食い違うよう goal を消す。
    let body = dir.join("matches/2026-01-03-tigers-vs-hawks.json");
    let mut dto: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&body).unwrap()).unwrap();
    dto["facts"].as_array_mut().unwrap().remove(1);
    std::fs::write(&body, serde_json::to_string(&dto).unwrap()).unwrap();

    let mut report = RunReport::default();
    validate_corpus(&dir, &mut report);
    let _ = std::fs::remove_dir_all(&dir);
    let codes = finding_codes(&report);
    assert!(
        codes.contains(&(
            "scoreMismatch".to_owned(),
            "2026-01-03-tigers-vs-hawks.json".to_owned()
        )),
        "大会の一覧の転記も本体と突き合わせるはず: {codes:#?}"
    );
    assert!(
        !codes.iter().any(|(code, _)| code == "orphanMatchFile"),
        "大会の一覧に載った本体は orphan ではない: {codes:#?}"
    );
}

/// corpus-ok を一時ディレクトリへ写し、試合の一覧を root 直下から `matches/index.json` へ移す
/// （3 つの一覧が `matches/` / `highlights/` / `competitions/` に揃う構成。handball-project#588）。
fn nested_layout_corpus(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "handball-toolkit-cli-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&fixture("corpus-ok"), &dir);
    std::fs::rename(dir.join("index.json"), dir.join("matches/index.json")).expect("rename");
    dir
}

/// 一覧を `matches/index.json` に置く構成も、root 直下に置く構成と同じに検証する。
/// `matches/index.json` を試合の本体として読まない（decode の指摘も orphan も出ない）ことも固定する。
#[test]
fn nested_match_index_layout_has_no_findings() {
    let dir = nested_layout_corpus("nested-ok");
    let mut report = RunReport::default();
    validate_corpus(&dir, &mut report);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        report.findings.len(),
        0,
        "指摘なしのはず: {:#?}",
        report.findings
    );
    assert_eq!(report.checked_files, 9);
}

/// 一覧がどちらの置き場にも無ければ、`matches/index.json` を名指して `missingIndex` を出し、
/// 本体の個別検証だけは続ける（`matches/` の中の一覧ではないファイルを本体として読む）。
#[test]
fn missing_match_index_points_to_nested_path() {
    let dir = nested_layout_corpus("nested-missing");
    std::fs::remove_file(dir.join("matches/index.json")).expect("remove");
    let mut report = RunReport::default();
    validate_corpus(&dir, &mut report);
    let _ = std::fs::remove_dir_all(&dir);
    let missing: Vec<&str> = report
        .findings
        .iter()
        .filter(|finding| {
            finding.issue.get("code").and_then(|v| v.as_str()) == Some("missingIndex")
        })
        .map(|finding| finding.path.as_str())
        .collect();
    assert_eq!(missing.len(), 1, "{:#?}", report.findings);
    assert!(
        Path::new(missing[0]).ends_with("matches/index.json"),
        "新しい構成の置き場を名指すはず: {missing:?}"
    );
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            std::fs::create_dir_all(&target).expect("create_dir_all");
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy");
        }
    }
}

#[test]
fn corpus_bad_reports_expected_findings() {
    let mut report = RunReport::default();
    validate_corpus(&fixture("corpus-bad"), &mut report);
    let codes = finding_codes(&report);

    // corpus-bad の違反はすべて意図的に仕込んである。fixture を「直す」ときは
    // どの検査の唯一の証拠を消すことになるかを確認すること。
    let expect = [
        ("duplicateSlug", "index.json"),
        // matches index が date 昇順に並んでいる（handball-project#115 の退行の形）。
        ("indexNotDateDescending", "index.json"),
        // highlights index の slug に先頭 yyyy-MM-dd が無い。
        ("slugDateMismatch", "index.json"),
        ("missingMatchFile", "2026-02-02-missing-file.json"),
        // matches index の 4 件目は slug に `..` と `/` を含む（handball-project#285）。
        // パスに埋める前に弾かれるので missingMatchFile ではなく index の指摘として出る。
        ("unsafeSlug", "index.json"),
        ("scoreMismatch", "2026-02-01-bad-score.json"),
        // facts[3] の factID が facts[1] と同じ。
        ("duplicateFactID", "2026-02-01-bad-score.json"),
        // facts[5] の possession が end < start（handball-project#220）。**facts[4] は
        // end > start の正常な possession** で、こちらは指摘されないことが
        // `possession_with_valid_end_is_not_flagged` の証拠になっている。
        ("possessionEndBeforeStart", "2026-02-01-bad-score.json"),
        ("orphanMatchFile", "orphan.json"),
        ("videoHighlightContainsPhaseStart", "with-phase.json"),
        ("factCountMismatch", "with-phase.json"),
        ("teamNameMismatch", "with-phase.json"),
        // `unexpectedAnchorEnd` は play だけの検査になった（#220 で possession を外した）。
        // この 1 件が唯一の証拠。
        ("unexpectedAnchorEnd", "with-phase.json"),
        // 大会（competitions/。handball-project#566）。index に同じ id が 2 つ / id に `..` と `/` /
        // 一覧ファイルが無い id / 大会 index に無い一覧ファイル。
        ("duplicateCompetitionID", "index.json"),
        ("unsafeCompetitionID", "index.json"),
        ("missingCompetitionFile", "no-file.json"),
        ("orphanCompetitionFile", "stray.json"),
        // bad-cup.json の 2 件目は index.json にも載っている slug。1 件目はチーム名の転記が無く、
        // 本体も無い（missingMatchFile は index.json 側の 1 件と同じ code で別ファイル）。
        ("slugListedTwice", "bad-cup.json"),
        ("missingTeamName", "bad-cup.json"),
        ("missingMatchFile", "2026-02-05-no-names.json"),
        // 大会の一覧の節と表（handball-project#567）。3 件目の「第2節」は 1 件目と離れている /
        // 表は 1 つ目の 2 行目のセル数が列と合わず、2 つ目が同じ id、3 つ目の id に空白、
        // 4 つ目に列が無い。index.json 側は `tables` を持ってはいけない。
        ("roundNotContiguous", "bad-cup.json"),
        ("tableRowWidthMismatch", "bad-cup.json"),
        ("duplicateTableID", "bad-cup.json"),
        ("unsafeTableID", "bad-cup.json"),
        ("tableWithoutColumns", "bad-cup.json"),
        ("unexpectedTables", "index.json"),
        // 大会の部門（handball-project#578）。部門が空の大会 / 一覧ファイルが無い部門 /
        // 部門を持つ大会の id の一覧ファイル（一覧は部門ごとなので名簿のどこからも引かれない）。
        // 大会と部門をまたいだ id の重複と字種は `corpus_bad_pins_competition_division_details`。
        ("emptyCompetitionDivisions", "index.json"),
        ("missingCompetitionFile", "no-division-file.json"),
        ("orphanCompetitionFile", "bad-league.json"),
        // 短い名前と表の凡例（handball-project#582）。bad-cup.json の 1 件目は正式名が無いのに短い名前を
        // 持ち、3 件目のアウェイの短い名前は空白だけ / 5 つ目の表は凡例の文字が重複し、意味が空のものがある。
        // どの行・どの文字かは `short_name_and_legend_details_are_pinned`。
        ("shortNameWithoutTeamName", "bad-cup.json"),
        ("emptyTeamShortName", "bad-cup.json"),
        ("duplicateLegendText", "bad-cup.json"),
        ("emptyLegendEntry", "bad-cup.json"),
    ];
    for (code, file) in expect {
        assert!(
            codes
                .iter()
                .any(|(found_code, found_file)| found_code == code && found_file == file),
            "{code} ({file}) が見つからない: {codes:#?}"
        );
    }

    // 負の videoClock は fact 単位の指摘として facts[2] / factID 付きで出る。
    let negative_clock = report
        .findings
        .iter()
        .find(|finding| {
            finding.path.ends_with("2026-02-01-bad-score.json")
                && finding.issue.get("scope").and_then(|value| value.as_str()) == Some("fact")
        })
        .expect("2026-02-01-bad-score.json に fact scope の指摘があるはず");
    assert_eq!(negative_clock.fact_index, Some(2));
    assert_eq!(
        negative_clock.fact_id.as_deref(),
        Some("44444444-4444-4444-4444-444444444444")
    );
}

/// 大会の部門（handball-project#578）の指摘を params まで固定する。
///
/// - 大会と部門の id は名簿全体で 1 つの名前空間（部門の id `bad-cup` は部門を持たない大会の
///   `bad-cup` と重なる）。大会どうしの重複と同じ code なので、id ごとの件数で区別する
/// - 部門の id もファイル名になるので字種を見る
#[test]
fn corpus_bad_pins_competition_division_details() {
    let mut report = RunReport::default();
    validate_corpus(&fixture("corpus-bad"), &mut report);

    let ids_of = |code: &str| -> Vec<String> {
        report
            .findings
            .iter()
            .filter(|finding| {
                finding.issue.get("code").and_then(|value| value.as_str()) == Some(code)
            })
            .filter_map(|finding| finding.issue["params"]["id"].as_str().map(str::to_owned))
            .collect()
    };

    let duplicates = ids_of("duplicateCompetitionID");
    assert_eq!(
        duplicates.iter().filter(|id| *id == "bad-cup").count(),
        2,
        "大会どうしの重複 1 件 + 部門と大会の重複 1 件のはず: {duplicates:#?}"
    );
    assert!(
        ids_of("unsafeCompetitionID").contains(&"bad league".to_owned()),
        "部門の id の字種も見るはず: {:#?}",
        report.findings
    );
    assert_eq!(ids_of("emptyCompetitionDivisions"), vec!["empty-league"]);
    assert!(
        ids_of("missingCompetitionFile").contains(&"no-division-file".to_owned()),
        "部門の一覧ファイルは部門の id で引くはず: {:#?}",
        report.findings
    );
}

/// 新しい 4 検査の params / fact 位置まで固定する。code が出ることだけを見ると、
/// 「どの要素が違反か」を取り違えた実装が通ってしまうため。
#[test]
fn corpus_bad_pins_new_check_details() {
    let mut report = RunReport::default();
    validate_corpus(&fixture("corpus-bad"), &mut report);

    let by_code = |code: &str, file: &str| {
        report
            .findings
            .iter()
            .find(|finding| {
                finding.issue.get("code").and_then(|value| value.as_str()) == Some(code)
                    && finding.path.ends_with(file)
            })
            .unwrap_or_else(|| panic!("{code} ({file}) が無い: {:#?}", report.findings))
    };

    // 降順違反は「昇順になっている隣接ペア」を名指しする。
    let order = by_code("indexNotDateDescending", "index.json");
    assert_eq!(
        order.issue["params"]["previousSlug"].as_str(),
        Some("2026-02-01-bad-score")
    );
    assert_eq!(
        order.issue["params"]["slug"].as_str(),
        Some("2026-02-02-missing-file")
    );

    // slug 先頭が日付ですらない場合も同じ code で、found に実際の先頭を載せる。
    let slug_date = by_code("slugDateMismatch", "index.json");
    assert_eq!(
        slug_date.issue["params"]["slug"].as_str(),
        Some("with-phase")
    );
    assert_eq!(
        slug_date.issue["params"]["expected"].as_str(),
        Some("2026-02-03")
    );
    assert_eq!(
        slug_date.issue["params"]["found"].as_str(),
        Some("with-phase")
    );

    // 経路離脱 slug は index.json の指摘で、slug をそのまま params に載せる。
    // 日付接頭辞は正しい（`2026-01-31`）ので slugDateMismatch とは別の違反であることも
    // ここで固定する（上の slugDateMismatch の pin は highlights 側の 1 件目を見ている）。
    let unsafe_slug = by_code("unsafeSlug", "index.json");
    assert_eq!(
        unsafe_slug.issue["params"]["slug"].as_str(),
        Some("2026-01-31-../escape")
    );

    // 重複 factID は 2 件目（後から現れた方）を facts[] index 付きで指す。
    let duplicate = by_code("duplicateFactID", "2026-02-01-bad-score.json");
    assert_eq!(duplicate.fact_index, Some(3));
    assert_eq!(
        duplicate.fact_id.as_deref(),
        Some("22222222-2222-2222-2222-222222222222")
    );

    // 逆順の possession end は facts[5] を名指しする（facts[4] の正常な end ではなく）。
    let possession_end = by_code("possessionEndBeforeStart", "2026-02-01-bad-score.json");
    assert_eq!(possession_end.fact_index, Some(5));
    assert_eq!(
        possession_end.fact_id.as_deref(),
        Some("55555555-5555-5555-5555-555555555555")
    );

    // `unexpectedAnchorEnd` は play にだけ効く。end を載せた側の値だけを params に置く。
    let play_end = by_code("unexpectedAnchorEnd", "with-phase.json");
    assert_eq!(
        play_end.issue["params"]["payloadKind"].as_str(),
        Some("play")
    );
    assert_eq!(play_end.fact_index, Some(1));
}

/// **end > start の possession は指摘されない**（handball-project#220）。
///
/// #220 以前は possession の anchor に end があるだけで `unexpectedAnchorEnd` が出ていた。
/// 検査を外したことが「possession の end に何の検査も無くなった」ではないこと
/// （逆順は `possessionEndBeforeStart` が捕まえる）と対で固定する。
#[test]
fn possession_with_valid_end_is_not_flagged() {
    let mut report = RunReport::default();
    validate_corpus(&fixture("corpus-bad"), &mut report);

    // facts[4] = videoClock 140 → end 160 の正常な possession。
    assert!(
        !report.findings.iter().any(|finding| {
            finding.path.ends_with("2026-02-01-bad-score.json") && finding.fact_index == Some(4)
        }),
        "facts[4] は正常な possession なので指摘されないはず: {:#?}",
        report.findings
    );
    // 検査自体が possession を素通りするようになったことも固定する。
    assert!(
        !report.findings.iter().any(|finding| {
            finding.issue.get("code").and_then(|value| value.as_str())
                == Some("unexpectedAnchorEnd")
                && finding
                    .issue
                    .get("params")
                    .and_then(|params| params.get("payloadKind"))
                    .and_then(|value| value.as_str())
                    == Some("possession")
        }),
        "possession は unexpectedAnchorEnd の対象外のはず: {:#?}",
        report.findings
    );
}

/// 正しく並んだ index には降順・slug 日付のどちらも出ない（偽陽性ゼロの確認）。
/// 同日の試合が複数ある配信コーパスは普通なので、同値の並びを含めて固定する。
#[test]
fn descending_index_with_same_date_entries_has_no_findings() {
    let mut report = RunReport::default();
    validate_file(&fixture("index-order-ok.json"), &mut report);
    assert_eq!(report.findings.len(), 0, "{:#?}", report.findings);
}

#[test]
fn single_match_file_mode_detects_shape() {
    let mut report = RunReport::default();
    validate_file(
        &fixture("corpus-ok/matches/2026-01-01-tigers-vs-falcons.json"),
        &mut report,
    );
    assert_eq!(report.findings.len(), 0, "{:#?}", report.findings);

    let mut report = RunReport::default();
    validate_file(&fixture("corpus-ok/index.json"), &mut report);
    assert_eq!(report.findings.len(), 0, "{:#?}", report.findings);
}

#[test]
fn half_only_match_reports_coverage_warning() {
    // regular PhaseStart が 1 件だけ（前半のみ相当）の試合本体。ドメイン整合は
    // 通るが「試合全体を覆っていない」warning が 1 件出る（handball-project#90）。
    let mut report = RunReport::default();
    validate_file(&fixture("half-only-match.json"), &mut report);

    assert_eq!(
        report.error_count(),
        0,
        "error は無いはず: {:#?}",
        report.findings
    );
    assert_eq!(
        report.warning_count(),
        1,
        "warning は 1 件: {:#?}",
        report.findings
    );

    let finding = report
        .findings
        .iter()
        .find(|finding| {
            finding.issue.get("code").and_then(|v| v.as_str()) == Some("matchCoverageIncomplete")
        })
        .expect("matchCoverageIncomplete が出るはず");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(
        finding.issue["params"]["regularPhaseCount"].as_i64(),
        Some(1)
    );
}

#[test]
fn full_match_has_no_coverage_warning() {
    // 前後半 2 phase の完全試合には warning が出ない（偽陽性ゼロの確認）。
    let mut report = RunReport::default();
    validate_file(
        &fixture("corpus-ok/matches/2026-01-01-tigers-vs-falcons.json"),
        &mut report,
    );
    assert_eq!(report.findings.len(), 0, "{:#?}", report.findings);
}

#[test]
fn warning_only_input_exits_zero() {
    // warning のみ（error なし）の入力は exit 0。severity が exit code を分ける。
    let bin = env!("CARGO_BIN_EXE_handball-toolkit-cli");
    let out = Command::new(bin)
        .args([
            "validate",
            "--json",
            fixture("half-only-match.json").to_str().unwrap(),
        ])
        .output()
        .expect("バイナリ実行に失敗");
    assert_eq!(out.status.code(), Some(0), "warning のみは exit 0: {out:?}");

    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("--json 出力は JSON のはず");
    let findings = parsed["findings"].as_array().expect("findings は配列");
    assert!(
        findings.iter().any(|finding| {
            finding["issue"]["code"].as_str() == Some("matchCoverageIncomplete")
                && finding["severity"].as_str() == Some("warning")
        }),
        "warning severity の matchCoverageIncomplete が出るはず: {parsed:#?}"
    );
}

#[test]
fn binary_exit_codes_and_json_output() {
    let bin = env!("CARGO_BIN_EXE_handball-toolkit-cli");

    let ok = Command::new(bin)
        .args(["validate", fixture("corpus-ok").to_str().unwrap()])
        .output()
        .expect("バイナリ実行に失敗");
    assert_eq!(ok.status.code(), Some(0), "{ok:?}");

    let bad = Command::new(bin)
        .args([
            "validate",
            "--json",
            fixture("corpus-bad").to_str().unwrap(),
        ])
        .output()
        .expect("バイナリ実行に失敗");
    assert_eq!(bad.status.code(), Some(1), "{bad:?}");
    let parsed: serde_json::Value =
        serde_json::from_slice(&bad.stdout).expect("--json 出力は JSON のはず");
    assert!(
        parsed["findings"]
            .as_array()
            .is_some_and(|findings| !findings.is_empty())
    );

    let usage = Command::new(bin)
        .args(["validate"])
        .output()
        .expect("バイナリ実行に失敗");
    assert_eq!(usage.status.code(), Some(2), "{usage:?}");
}

/// 大会の一覧の節と表の指摘は、どの行・どの表が違反かまで固定する（handball-project#567）。
#[test]
fn competition_round_and_table_details_are_pinned() {
    let mut report = RunReport::default();
    validate_corpus(&fixture("corpus-bad"), &mut report);
    let by_code = |code: &str, file: &str| {
        report
            .findings
            .iter()
            .find(|finding| {
                finding.issue.get("code").and_then(|value| value.as_str()) == Some(code)
                    && finding.path.ends_with(file)
            })
            .unwrap_or_else(|| panic!("{code} ({file}) が無い: {:#?}", report.findings))
    };

    // 離れて現れた 2 つ目のまとまりの先頭の行を名指しする（1 つ目の「第2節」は指摘しない）。
    let round = by_code("roundNotContiguous", "bad-cup.json");
    assert_eq!(
        round.issue["params"]["slug"].as_str(),
        Some("2026-02-03-round-again")
    );
    assert_eq!(round.issue["params"]["round"].as_str(), Some("第2節"));
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| {
                finding.issue.get("code").and_then(|value| value.as_str())
                    == Some("roundNotContiguous")
            })
            .count(),
        1
    );

    let width = by_code("tableRowWidthMismatch", "bad-cup.json");
    assert_eq!(width.issue["params"]["id"].as_str(), Some("standings"));
    assert_eq!(width.issue["params"]["row"].as_u64(), Some(1));
    assert_eq!(width.issue["params"]["expected"].as_u64(), Some(2));
    assert_eq!(width.issue["params"]["found"].as_u64(), Some(1));
}

/// 大会の一覧は配信側が節の順に並べるので、`date` 降順を求めるのは同じ節の中だけ
/// （handball-project#567）。節をまたいで日付が戻るのは違反ではなく、同じ節の中で
/// 日付が進むのは従来どおり違反。
#[test]
fn date_descending_is_checked_within_a_round_only() {
    use chrono::{TimeZone, Utc};
    use handball_toolkit::sample_dto::{SampleIndexDtoV2, SampleMatchSummaryV2};
    use handball_toolkit_cli::validate::validate_match_index_dto;

    let entry = |slug: &str, day: u32, round: &str| SampleMatchSummaryV2 {
        slug: slug.to_owned(),
        display_name: "A vs B".to_owned(),
        description: None,
        date: Utc.with_ymd_and_hms(2026, 1, day, 10, 0, 0).unwrap(),
        home_score: 1,
        away_score: 0,
        has_video: false,
        home_team_name: Some("A".to_owned()),
        away_team_name: Some("B".to_owned()),
        round: Some(round.to_owned()),
        home_team_short_name: None,
        away_team_short_name: None,
    };

    // 第2節（1/3）→ 第1節（1/5・延期分が後の日付）。一覧全体では昇順の箇所があるが違反ではない。
    let ordered_by_round = SampleIndexDtoV2 {
        schema_version: 2,
        matches: vec![
            entry("2026-01-03-a-vs-b", 3, "第2節"),
            entry("2026-01-05-a-vs-b", 5, "第1節"),
            entry("2026-01-01-a-vs-b", 1, "第1節"),
        ],
        tables: None,
    };
    let mut report = RunReport::default();
    validate_match_index_dto("list.json", &ordered_by_round, &mut report);
    assert!(report.findings.is_empty(), "{:#?}", report.findings);

    // 同じ節の中で日付が進むのは違反。
    let ascending_in_round = SampleIndexDtoV2 {
        schema_version: 2,
        matches: vec![
            entry("2026-01-01-a-vs-b", 1, "第1節"),
            entry("2026-01-05-a-vs-b", 5, "第1節"),
        ],
        tables: None,
    };
    let mut report = RunReport::default();
    validate_match_index_dto("list.json", &ascending_in_round, &mut report);
    let codes: Vec<_> = finding_codes(&report)
        .into_iter()
        .map(|(code, _)| code)
        .collect();
    assert_eq!(codes, vec!["indexNotDateDescending".to_owned()]);
}

/// 短い名前と表の凡例（handball-project#582）の指摘を、どの項目・どの文字かまで固定する。
///
/// - 短い名前の指摘は項目の slug と側（home / away）を名指しする
/// - 凡例の重複は 2 つ目だけを指摘する（1 つ目は正しい）。意味が空の項目は重複とは別の code
#[test]
fn short_name_and_legend_details_are_pinned() {
    let mut report = RunReport::default();
    validate_corpus(&fixture("corpus-bad"), &mut report);
    let all = |code: &str| {
        report
            .findings
            .iter()
            .filter(|finding| {
                finding.issue.get("code").and_then(|value| value.as_str()) == Some(code)
            })
            .map(|finding| finding.issue["params"].clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        all("shortNameWithoutTeamName"),
        vec![serde_json::json!({"slug": "2026-02-05-no-names", "side": "home"})]
    );
    assert_eq!(
        all("emptyTeamShortName"),
        vec![serde_json::json!({"slug": "2026-02-03-round-again", "side": "away"})]
    );
    assert_eq!(
        all("duplicateLegendText"),
        vec![serde_json::json!({"id": "legend", "text": "T"})]
    );
    assert_eq!(
        all("emptyLegendEntry"),
        vec![serde_json::json!({"id": "legend", "text": "H"})]
    );
}
