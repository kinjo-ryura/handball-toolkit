//! 移植元: アプリ層 `SampleMatches/V2/SampleMatchDTOsV2.swift`。
//!
//! V2 sample 配信 DTO（handball-match-data の配信 JSON の schema）。`SAMPLE_DTO_V2.md` 準拠。
//!
//! V1 schema との主な差分:
//! - `MatchConfiguration` は tagged union（`kind` discriminator + sub-payload）
//! - 旧 `phaseRules` / `MatchPhase.timeline` は廃止（phase 情報は PhaseStart fact から再現）
//! - `ControlFact` は 2 case sum type（`phaseStart` / `stoppage`）
//! - `MatchClock` は試合通算累積秒（phase 内秒数ではない）
//!
//! tagged union は Swift 実装と同じ「`kind` 文字列 + optional 兄弟フィールド」のプレーン
//! struct で表現する（serde の enum タグ化はしない）。kind 判定と payload の突合は
//! converter 側で行い、未知値はデコード段階で落とさず `SampleMatchDecodeErrorV2` として
//! 構造化報告する Swift の責務分割を保存するため。
//!
//! serialize は Swift `SampleMatchEncoderV2`（JSONEncoder）の出力とバイト一致させる:
//! nil キー省略（`skip_serializing_if`）・UUID 大文字・秒未満切り捨て（`swift_wire`）を
//! 本ファイルで、整形（pretty / sortedKeys / `/` 非エスケープ）は `sample_match_encoder` が
//! 担う。バイト正は `tests/golden/export/`（ADR 0004 決定 2）。deserialize は従来どおり
//! 寛容（明示 null / UUID 小文字 / RFC 3339 を受理 — 実配信コーパスは python 生成で形式が違う）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `schemaVersion` の現行値。移植元: `SampleMatchSchemaVersionV2.current`。
pub const SCHEMA_VERSION_CURRENT: i64 = 2;

// ── Index ──

/// `matches/index.json` トップレベル構造。大会の一覧（`competitions/{id}.json`）も同じ形で、
/// 大会だけが `tables` を持つ（handball-project#567）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleIndexDtoV2 {
    pub schema_version: i64,
    pub matches: Vec<SampleMatchSummaryV2>,
    /// 大会ごとの表（順位表・得点ランキング・星取表など）。**大会の一覧だけ**が持ち、
    /// `matches/index.json` には無い（検証 CLI の `unexpectedTables`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub tables: Option<Vec<SampleTableV2>>,
}

/// `matches/index.json` と `competitions/{id}.json` の `matches[]` 要素。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchSummaryV2 {
    pub slug: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(with = "swift_wire::iso8601_seconds")]
    pub date: DateTime<Utc>,
    pub home_score: i64,
    pub away_score: i64,
    pub has_video: bool,
    /// 本体のチーム名の転記（handball-project#566）。**optional** — 大会の一覧
    /// （`competitions/{id}.json`）では必須で、本体を取らずに行を組むために使う。
    /// `matches/index.json` の既存の項目には無い（旧アプリは本体から名前を引く）。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub home_team_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub away_team_name: Option<String>,
    /// 節 / 回戦の表示ラベル（handball-project#567。例: `"第1節"` / `"準決勝"`）。大会の一覧
    /// でだけ使い、アプリは一覧の並びのまま**同じラベルが続く行を 1 つのセクション**にまとめる。
    /// 文言は配信側が組み、アプリは語を足さない。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub round: Option<String>,
    /// ホームの短い名前（公式の略称など。handball-project#582。例: `"TMEJ"`）。**optional** —
    /// アプリは一覧の行で正式名（`home_team_name`）の代わりに出し、無ければ正式名。配信側は
    /// 正式名と同じ短い名前を書かない。書くなら `home_team_name` も要る（検証 CLI の
    /// `shortNameWithoutTeamName`）。試合本体のチームには無い（試合画面は正式名のまま）。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub home_team_short_name: Option<String>,
    /// アウェイの短い名前。`home_team_short_name` と同じ規則。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub away_team_short_name: Option<String>,
}

// ── Tables（大会の表）──

/// 大会の一覧（`competitions/{id}.json`）の `tables[]` 要素（handball-project#567）。
///
/// 順位表・得点ランキング・星取表のような大会ごとの表を、**列の意味を持たない汎用の表**として
/// 配信する。アプリは題・列見出し・寄せ・セルの文字列をそのまま描くだけで、表の種類が増えても
/// 改修しない。中身の計算と並び（順位の決め方）は配信側の tool が持つ。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTableV2 {
    /// 表の識別子（英数字と `-`）。同じ一覧の中で一意。
    pub id: String,
    /// 表の題（例: `"順位表"`）。アプリは切替のラベルに出す。
    pub title: String,
    /// 列の定義。1 列以上。
    pub columns: Vec<SampleTableColumnV2>,
    /// 行。各行のセル数は `columns` の数と一致する。セルは表示する文字列そのまま。
    pub rows: Vec<Vec<String>>,
    /// 凡例（handball-project#582）。セルや列見出しに書いた略した文字（チームの短い名前など）と
    /// その意味。**optional** — アプリは凡例にある文字と完全に一致するセル・列見出しをタップされたら
    /// 意味を出す。列の意味は持たない（どのセルがチーム名かはアプリに渡さない）。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub legend: Option<Vec<SampleTableLegendEntryV2>>,
}

/// `tables[].legend[]` 要素（handball-project#582）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTableLegendEntryV2 {
    /// セル・列見出しに書いた文字（例: `"TMEJ"`）。表の中で一意（検証 CLI の `duplicateLegendText`）。
    pub text: String,
    /// 意味（例: `"トヨタ自動車東日本レガロッソ宮城"`）。
    pub detail: String,
}

/// `tables[].columns[]` 要素。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTableColumnV2 {
    pub title: String,
    /// 寄せ。省けば `leading`。数値の列は `trailing`。
    #[serde(default)]
    pub align: SampleTableAlignV2,
}

/// 表の列の寄せ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum SampleTableAlignV2 {
    #[default]
    Leading,
    Trailing,
    Center,
}

// ── Competitions（大会）──

/// `competitions/index.json` トップレベル構造（handball-project#566）。
///
/// 大会は「注目の試合」（`matches/index.json`）とは別に配信する試合のまとまり。本体は
/// `matches/{slug}.json` を共有し、1 つの本体は `matches/index.json` か 1 つの大会の
/// どちらか一方にだけ載る。大会の試合一覧は `competitions/{id}.json` で、形は
/// `matches/index.json` と同じ（`SampleIndexDtoV2`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleCompetitionIndexDtoV2 {
    pub schema_version: i64,
    pub competitions: Vec<SampleCompetitionSummaryV2>,
}

/// `competitions/index.json` の `competitions[]` 要素。アプリはこの 1 つにバナーを 1 本出す。
///
/// **部門**（男子・女子・年代など。handball-project#578）を持つ大会は、順位表と節が部門ごとに
/// 別なので、試合一覧のファイルを部門ごとに持つ（`competitions/{部門の id}.json`）。部門を持たない
/// 大会は大会の id がそのまま一覧ファイルの名前になる。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleCompetitionSummaryV2 {
    /// 大会の識別子。部門を持たなければ `competitions/{id}.json` のファイル名（拡張子除く）と
    /// 完全一致。slug と同じ字種（英数字と `-`）で、`index` は予約語。部門の id とも重ならない。
    pub id: String,
    /// 画面に出す大会名（例: `"リーグH 2026-27"`）。
    pub title: String,
    /// 1 文の説明。無ければ省く。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub description: Option<String>,
    /// 部門。配列順がアプリの切替の順。無ければ部門を持たない大会で、空の配列は書かない
    /// （検証 CLI の `emptyCompetitionDivisions`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub divisions: Option<Vec<SampleCompetitionDivisionV2>>,
}

/// `competitions[].divisions[]` 要素（handball-project#578）。部門 1 つが試合一覧 1 つ・表 1 組になる。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleCompetitionDivisionV2 {
    /// `competitions/{id}.json` のファイル名（拡張子除く）と完全一致。字種と予約語は大会の id と
    /// 同じで、名簿の中の大会と部門の id 全体で一意。
    pub id: String,
    /// 切替に出す部門名（例: `"男子"`）。大会名は繰り返さない。
    pub title: String,
}

impl SampleCompetitionSummaryV2 {
    /// この大会の試合一覧ファイルの id（`competitions/{id}.json`）。部門があれば部門の id を
    /// 配列順に、無ければ大会の id 1 つ。
    pub fn listing_ids(&self) -> Vec<&str> {
        match &self.divisions {
            Some(divisions) => divisions.iter().map(|d| d.id.as_str()).collect(),
            None => vec![self.id.as_str()],
        }
    }
}

/// `highlights/index.json` トップレベル構造。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleHighlightIndexDtoV2 {
    pub schema_version: i64,
    pub highlights: Vec<SampleHighlightSummaryV2>,
}

/// `highlights/index.json` の `highlights[]` 要素。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleHighlightSummaryV2 {
    pub slug: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(with = "swift_wire::iso8601_seconds")]
    pub date: DateTime<Utc>,
    pub home_team_name: String,
    pub away_team_name: String,
    pub fact_count: i64,
    pub has_video: bool,
}

// ── Match body ──

/// `matches/{slug}.json` および `highlights/{slug}.json` の本体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchDtoV2 {
    pub schema_version: i64,
    /// このファイルを書いたアプリ（handball-project#298 / #300）。**optional** — 配信サンプル
    /// （python 生成）と 1.6.0 が書いた試合ファイルには無い。サポートで「外部生成」と
    /// 「自アプリ + version 差」を切り分けるためのもので、改竄防止ではない（Recorder ADR 0002）。
    /// コアは値を入れない（アプリ名 / version を知らない）— export 後にシェルが埋める。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub generator: Option<SampleGeneratorDtoV2>,
    pub r#match: SampleMatchHeaderV2,
    pub teams: SampleTeamsDtoV2,
    pub facts: Vec<SampleFactDtoV2>,
}

/// `generator` — ファイルを書いたアプリの名前と版。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleGeneratorDtoV2 {
    /// アプリ名（例: `"HandballRecorder"`）。
    pub name: String,
    /// 利用者に見える版（例: `"1.6.1"`）。
    pub version: String,
    /// ビルド番号（例: `"29"`）。持たない生成元は省略する。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub build: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchHeaderV2 {
    /// ユーザーが付けた試合タイトル。None 許容（`Match.title` が optional）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(with = "swift_wire::iso8601_seconds")]
    pub date: DateTime<Utc>,
    pub configuration: SampleMatchConfigurationDtoV2,
}

// ── Teams / Players ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTeamsDtoV2 {
    pub home: SampleTeamDtoV2,
    pub away: SampleTeamDtoV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTeamDtoV2 {
    pub key: String,
    pub name: String,
    pub players: Vec<SamplePlayerDtoV2>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SamplePlayerDtoV2 {
    pub key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jersey_number: Option<i64>,
}

// ── Configuration (tagged union) ──

/// `MatchConfiguration` の JSON 表現（tagged union pattern）。
///
/// `kind` discriminator で case を切り替え、対応する sub-payload struct を読む。
/// - `timer` → `timer: SampleTimerConfigurationDtoV2`
/// - `video` → `video: SampleVideoConfigurationDtoV2`
/// - `videoHighlight` → `videoHighlight: SampleVideoConfigurationDtoV2`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchConfigurationDtoV2 {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timer: Option<SampleTimerConfigurationDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<SampleVideoConfigurationDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_highlight: Option<SampleVideoConfigurationDtoV2>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleTimerConfigurationDtoV2 {
    pub phase_duration_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleVideoConfigurationDtoV2 {
    pub source: SampleVideoSourceDtoV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleVideoSourceDtoV2 {
    /// `"youtube"` | `"local"`。配信サンプルは `"youtube"` のみ、`"local"` は試合ファイル
    /// （端末内「写真」の動画。`externalID` は PHAsset の localIdentifier で端末固有）。
    pub provider: String,
    /// Swift 表記 `externalID` を保存（camelCase 自動変換は `externalId` になるため明示 rename）。
    #[serde(rename = "externalID")]
    pub external_id: String,
    /// `"local"` のみ。端末をまたいで安定な PhotoKit の cloud identifier
    /// （handball-project#300）。受け取った端末は iCloud 写真が同じ Apple ID で有効なら、
    /// これをその端末の localIdentifier へ引き直して再生を復旧する。**optional** —
    /// 1.6.0 が書いたファイルには無く、送り側で引けなかったときも省略される。
    /// ドメイン型（`VideoSource`）には持たせない — 参照の復旧はシェルの仕事で、
    /// fact / projection には関与しない。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub cloud_identifier: Option<String>,
    /// `"local"` のみ。送り側で紐付いていた動画の尺（秒）。受け取った端末で動画を選び直す
    /// とき、別の切り出しへ差し替えて fact の時刻が静かにずれるのを止めるために比べる
    /// （Recorder `CONTEXT.md`「動画ソースの差し替えは移行と別の操作」の条件 (b)）。
    /// **optional** — `cloud_identifier` と同じ理由。
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub duration_seconds: Option<f64>,
}

// ── Facts (tagged union) ──

/// 1 件の事実。payload は play / control の tagged union。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleFactDtoV2 {
    /// 永続化されている UUID（なければ converter 側で採番。ID 供給はシェル注入 — converter 参照）。
    #[serde(
        rename = "factID",
        default,
        with = "swift_wire::uuid_uppercase_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub fact_id: Option<Uuid>,
    #[serde(with = "swift_wire::iso8601_seconds")]
    pub recorded_at: DateTime<Utc>,
    pub payload: SampleFactPayloadDtoV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleFactPayloadDtoV2 {
    /// `"play"` | `"control"` | `"possession"`
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play: Option<SamplePlayFactDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<SampleControlFactDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub possession: Option<SamplePossessionFactDtoV2>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SamplePlayFactDtoV2 {
    /// `PlayEventKind` raw value（`goal` / `shotMissed` / `freeNote` / `yellowCard` / `twoMinuteSuspension` / `redCard`）。
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_player_key: Option<String>,
    pub anchor: SampleFactAnchorDtoV2,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleControlFactDtoV2 {
    /// `"phaseStart"` | `"stoppage"`
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_start: Option<SamplePhaseStartPayloadDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stoppage: Option<SampleStoppagePayloadDtoV2>,
    /// 開始 anchor。end 情報は `anchor.end_match_elapsed_seconds` / `anchor.end_video_elapsed_seconds`。
    pub anchor: SampleFactAnchorDtoV2,
}

/// ポゼッション開始（handball-project#154）。
///
/// 判別子（`kind`）を持たない — ポゼッションの fact は「開始」1 種類だけで、2 つ目が出た時点で
/// どのみちスキーマ変更になるため、空の判別子を先置きしない（handball-match-data の `SCHEMA.md` 参照）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SamplePossessionFactDtoV2 {
    /// `"home"` | `"away"`。**必須**（`SamplePlayFactDtoV2::team_key` と違い `Option` ではない）。
    pub team_key: String,
    /// そのチームのプレーが動き出した瞬間（handball-project#220 で「ボールを保持した瞬間」から
    /// 改めた）。end 系（`end_match_elapsed_seconds` / `end_video_elapsed_seconds`）は**任意** —
    /// 供給源が終わりを出せた区間にだけ入り、無ければ goal / 次のポゼッション開始 / phase end から
    /// 導出する。
    pub anchor: SampleFactAnchorDtoV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SamplePhaseStartPayloadDtoV2 {
    /// `PhaseKind` raw value（`regular` | `shootout`）。
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleStoppagePayloadDtoV2 {
    /// `StoppageKind` raw value（`timeout` | `pause`）。
    pub stoppage_kind: String,
    /// pause の自由記述（timeout では None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ── Anchor ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleFactAnchorDtoV2 {
    /// `FactAnchorKind` raw value（`matchClock` | `videoClock` | `both`）。
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_clock: Option<SampleMatchClockDtoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_clock: Option<SampleVideoClockDtoV2>,
    /// PhaseStart（必須）/ Stoppage・Possession（任意）の end（range 末尾）。PlayFact では両方 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_match_elapsed_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_video_elapsed_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleMatchClockDtoV2 {
    /// 試合通算 matchClock 累積秒数。
    pub elapsed_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SampleVideoClockDtoV2 {
    pub elapsed_seconds: f64,
}

// ── Swift JSONEncoder 互換の wire 表現 ──

/// Swift `JSONEncoder`（`.iso8601` / Codable の UUID encode）と一致する serialize と、
/// 従来どおり寛容な deserialize の組。バイト正は `tests/golden/export/`。
pub(crate) mod swift_wire {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    /// Swift `.iso8601` 相当: 秒未満は**切り捨て**（`.999` も丸め上げない）・`Z` 表記。
    pub(crate) mod iso8601_seconds {
        use super::*;

        pub fn serialize<S: Serializer>(
            value: &DateTime<Utc>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.serialize_str(&value.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<DateTime<Utc>, D::Error> {
            DateTime::<Utc>::deserialize(deserializer)
        }
    }

    /// Swift Codable の UUID encode 相当: 大文字ハイフン表記（デコードは大小不問）。
    pub(crate) mod uuid_uppercase_opt {
        use super::*;

        pub fn serialize<S: Serializer>(
            value: &Option<Uuid>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match value {
                Some(id) => serializer.serialize_str(&id.to_string().to_uppercase()),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Uuid>, D::Error> {
            Option::<Uuid>::deserialize(deserializer)
        }
    }
}

// ── Errors ──

/// DTO → domain 変換の失敗。移植元: `SampleMatchDecodeErrorV2`。
///
/// エラーコード + パラメータのみの構造化エラー（設計不変条件 3。文言はシェル所有）。
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum SampleMatchDecodeErrorV2 {
    SchemaVersionMismatch { found: i64, expected: i64 },
    UnknownConfigurationKind(String),
    MissingConfigurationPayload(String),
    UnknownPayloadKind(String),
    MissingPayloadBody(String),
    UnknownPlayKind(String),
    UnknownControlKind(String),
    UnknownStoppageKind(String),
    UnknownPhaseKind(String),
    UnknownAnchorKind(String),
    UnknownVideoProvider(String),
    MissingAnchorBody(String),
    UnknownTeamKey(String),
    UnknownPlayerKey(String),
    MissingPhaseStartEnd,
}
