//! validators。移植元: `Validators/` ディレクトリ（ADR 0001 ミラー表）。
//!
//! Swift の名前空間 enum（`FactValidator` 等）は Rust ではモジュール + 自由関数で表現する
//! （ADR 0001）。入力は借用、出力は所有値。`facts` は永続化順
//! （累積秒 → recordedAt → id）でソート済みである前提（入力契約 — ADR 0001）。

mod configuration_validator;
mod fact_log_validator;
mod fact_validator;
mod match_validator;
mod match_write_validator;

pub use configuration_validator::validate_configuration;
pub use fact_log_validator::validate_fact_log;
pub use fact_validator::{
    RosterContext, validate_control_fact, validate_match_fact, validate_play_fact,
    validate_possession_fact,
};
pub use match_validator::validate_match;
pub use match_write_validator::{validate_append, validate_delete, validate_update};
