//! sample-matches 配信 JSON（SAMPLE_DTO_V2）の検証 CLI（handball-project#58）。
//!
//! research doc「CLI / サーバーレス関数」出口の最初の一歩。コアの validators を
//! そのまま呼ぶだけの薄いシェルであり、コアには一切手を入れない（CLAUDE.md の
//! 拡張方針）。エラー文言はシェル所有の原則（設計不変条件 3）に従い、コアの
//! 構造化エラー（`{scope, code, params}` ワイヤ形式）をそのまま表示・出力する。
//!
//! - 単一ファイル: トップレベルキーで形状を自動判別（`facts` = 試合本体 /
//!   `matches` = 試合 index / `highlights` = ハイライト index）
//! - ディレクトリ（v2 ルート）: index ↔ ファイルの突合と SCHEMA.md 由来の
//!   整合チェック（スコア転記 / factCount / hasVideo / date）を含む一括検証

pub mod corpus;
pub mod report;
pub mod validate;
