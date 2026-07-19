//! CLI の統合テスト。fixtures のミニコーパス（corpus-ok / corpus-bad）を
//! ライブラリ API と実バイナリの両方で検証する。

use std::path::{Path, PathBuf};
use std::process::Command;

use handball_toolkit_cli::corpus::validate_corpus;
use handball_toolkit_cli::report::RunReport;
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

    let expect = [
        ("duplicateSlug", "index.json"),
        ("missingMatchFile", "missing-file.json"),
        ("scoreMismatch", "bad-score.json"),
        ("orphanMatchFile", "orphan.json"),
        ("videoHighlightContainsPhaseStart", "with-phase.json"),
        ("factCountMismatch", "with-phase.json"),
        ("teamNameMismatch", "with-phase.json"),
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
            finding.path.ends_with("bad-score.json")
                && finding.issue.get("scope").and_then(|value| value.as_str()) == Some("fact")
        })
        .expect("bad-score.json に fact scope の指摘があるはず");
    assert_eq!(negative_clock.fact_index, Some(2));
    assert_eq!(
        negative_clock.fact_id.as_deref(),
        Some("44444444-4444-4444-4444-444444444444")
    );
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
