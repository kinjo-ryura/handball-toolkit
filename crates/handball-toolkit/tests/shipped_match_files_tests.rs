//! 出荷したアプリが書いた試合ファイルを読み続ける約束の回帰テスト（handball-project#300）。
//!
//! `tests/fixtures/shipped/*.handrec` は出荷ビルドの実出力（README 参照）。ここが落ちたら
//! 「過去の利用者ファイルが読めなくなる変更」を入れている。直すのは fixture ではなくコード
//! （破壊的変更なら `schemaVersion` を上げ、旧版の読み込み経路を残す — Recorder ADR 0002 規律 1）。

use std::fs;
use std::path::{Path, PathBuf};

use handball_toolkit::sample_dto::{
    SCHEMA_VERSION_CURRENT, SampleMatchDtoV2, convert, encode_sample_match,
};
use handball_toolkit::validators::{validate_fact_log, validate_match};
use uuid::Uuid;

fn shipped_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shipped");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: 読めない: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "handrec"))
        .collect();
    files.sort();
    files
}

/// 対象 0 件を正常終了にしない — fixture を置き忘れたまま緑が続くのが一番危ない。
#[test]
fn shipped_fixtures_exist() {
    assert!(
        !shipped_files().is_empty(),
        "tests/fixtures/shipped/ に .handrec が 1 つも無い（README の置き方を参照）"
    );
}

/// 出荷ビルドの実出力を parse → convert → validation まで通す。
#[test]
fn shipped_match_files_still_import() {
    for path in shipped_files() {
        let label = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path).unwrap();
        let dto: SampleMatchDtoV2 = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{label}: parse に失敗: {error}"));
        assert!(
            dto.schema_version <= SCHEMA_VERSION_CURRENT,
            "{label}: 出荷済みファイルの schemaVersion {} が現行 {SCHEMA_VERSION_CURRENT} より新しい",
            dto.schema_version
        );

        let mut counter: u128 = 0;
        let conversion = convert(&label, &dto, None, || {
            counter += 1;
            Uuid::from_u128(counter)
        })
        .unwrap_or_else(|error| panic!("{label}: convert に失敗: {error:?}"));

        let issues = validate_match(&conversion.r#match);
        assert!(issues.is_empty(), "{label}: match validation: {issues:?}");
        let issues = validate_fact_log(&conversion.facts, &conversion.r#match);
        assert!(
            issues.is_empty(),
            "{label}: fact log validation: {issues:?}"
        );
    }
}

/// 出荷ビルドは Rust の `encode_sample_match` で書いているので、parse → 再 encode で
/// **バイト一致**する。optional フィールドの追加で旧ファイルの書式が変わっていないことの証明。
#[test]
fn shipped_match_files_round_trip_byte_for_byte() {
    for path in shipped_files() {
        let label = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path).unwrap();
        let dto: SampleMatchDtoV2 = serde_json::from_str(&text).unwrap();
        assert_eq!(
            encode_sample_match(&dto),
            text,
            "{label}: 再 encode がバイト一致しない"
        );
    }
}
