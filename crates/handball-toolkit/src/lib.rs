//! ハンドボール試合データのツールキット。
//!
//! HandballRecorder の `RecorderDomain`（Swift）を移植した stateless 純粋関数コア。
//! 公開 API はすべて「fact 列 in → 導出結果 out」の純粋関数で、
//! 時間・乱数・I/O・永続化は持たない（timestamp / ID はシェルが発行して fact に載せて渡す）。
//!
//! モジュール構成は移植元の Swift ディレクトリ構成を 1:1 でミラーする（ADR 0001 ミラー表）。
//!
//! 設計方針の背景: handball-project#49 と
//! `handball-project/docs/research/handballrecorder-rust-core.md` を参照。

pub mod clock;
pub mod configuration;
pub mod entities;
pub mod facts;
#[cfg(feature = "uniffi")]
pub mod ffi_api;
#[cfg(feature = "uniffi")]
mod ffi_support;
pub mod ids;
pub mod projection;
pub mod sample_dto;
pub mod validation;
pub mod validators;

// uniffi メタデータ（namespace: handball_toolkit）。型 derive も関数公開（ffi_api）も
// この 1 namespace に集約する — ADR 0004（複数 namespace で module_name を共有すると
// 生成ファイルが上書き衝突するため）。staticlib 化は handball-toolkit-ffi crate。
#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
