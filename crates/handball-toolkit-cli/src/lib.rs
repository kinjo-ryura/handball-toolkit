//! handball-match-data の配信 JSON（SAMPLE_DTO_V2）の検証 CLI（handball-project#58）。
//!
//! research doc「CLI / サーバーレス関数」出口の最初の一歩。コアの validators を
//! そのまま呼ぶだけの薄いシェルであり、コアには一切手を入れない（CLAUDE.md の
//! 拡張方針）。エラー文言はシェル所有の原則（設計不変条件 3）に従い、コアの
//! 構造化エラー（`{scope, code, params}` ワイヤ形式）をそのまま表示・出力する。
//!
//! - 単一ファイル: トップレベルキーで形状を自動判別（`facts` = 試合本体 /
//!   `matches` = 試合 index（大会の一覧も同じ形）/ `highlights` = ハイライト index /
//!   `competitions` = 大会 index）
//! - ディレクトリ（配信データのルート。一覧は `matches/index.json` か root の `index.json`）:
//!   index ↔ ファイルの突合と SCHEMA.md 由来の
//!   整合チェック（スコア転記 / factCount / hasVideo / date / チーム名）を含む一括検証。
//!   大会（`competitions/`）の一覧は `index.json` と本体を共有するので突合に合流させる
//!
//! SCHEMA.md の不変条件のうち、コアの validators が見ない「配列・ファイル群と
//! しての整合」は CLI 側で検査する（`corpus` scope）。index の slug 重複 /
//! date 降順 / slug 先頭日付、試合内の factID 重複、play・possession の
//! anchor end 系など。

pub mod corpus;
pub mod report;
pub mod validate;
