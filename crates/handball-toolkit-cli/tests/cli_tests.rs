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
    // index + match + highlight index + highlight = 4 ファイル
    assert_eq!(report.checked_files, 4);
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
