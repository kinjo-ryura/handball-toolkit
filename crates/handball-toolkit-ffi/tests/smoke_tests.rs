//! FFI 公開関数のスモークテスト（FFI を越える前に Rust 内で挙動を固定する）。
//! 入力・期待値ともコアのゴールデンコーパスを流用する。

use std::fs;
use std::path::PathBuf;

fn golden_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../handball-toolkit/tests/golden")
}

#[test]
fn バージョン文字列を返す() {
    assert_eq!(handball_toolkit_ffi::toolkit_version(), "0.1.0");
}

#[test]
fn ゴールデン入力の_summary_がオラクル期待値と一致する() {
    let slug = "2025-12-20-f352ea46";
    let input = fs::read_to_string(golden_root().join(format!("inputs/matches/{slug}.json")))
        .expect("golden 入力を読める");
    let expected: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(golden_root().join(format!("expected/matches/{slug}.json")))
            .expect("golden 期待値を読める"),
    )
    .expect("golden 期待値は JSON");

    let response: serde_json::Value = serde_json::from_str(
        &handball_toolkit_ffi::summarize_sample_match(input).expect("変換と集計に成功する"),
    )
    .expect("応答は JSON");

    assert_eq!(response["homeScore"], expected["summary"]["homeScore"]);
    assert_eq!(response["awayScore"], expected["summary"]["awayScore"]);
    assert_eq!(
        response["homeTeam"]["shotAttempts"],
        expected["summary"]["homeTeam"]["shotAttempts"]
    );
    assert_eq!(
        response["awayTeam"]["scoringRate"],
        expected["summary"]["awayTeam"]["scoringRate"]
    );
}

#[test]
fn 壊れた_json_は_invalid_json_エラーになる() {
    let error = handball_toolkit_ffi::summarize_sample_match("{".to_string())
        .expect_err("パース失敗はエラー");
    assert!(matches!(
        error,
        handball_toolkit_ffi::ToolkitError::InvalidJson { .. }
    ));
}
