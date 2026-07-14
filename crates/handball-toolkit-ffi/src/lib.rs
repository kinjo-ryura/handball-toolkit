//! UniFFI バインディング層（handball-project#49 の UniFFI PoC）。
//!
//! コア crate（handball-toolkit）の型付き API には手を入れず、この crate が
//! シェル向けの粗い境界「SAMPLE_DTO_V2 JSON in → summary JSON out」を提供する。
//! serde 層を境界の外側（バインディング側）に置くのは ADR 0001 の決定どおり。
//!
//! ドメイン全型を UniFFI 型へ写す本設計は Android シェル実装時の課題として持ち越し、
//! ここでは「Rust コア → Swift バインディング → XCFramework → iOS」の経路実証に徹する。

uniffi::setup_scaffolding!();

use handball_toolkit::projection::SummaryProjection;
use handball_toolkit::sample_dto::{SampleMatchDtoV2, convert};
use uuid::Uuid;

/// FFI 境界のエラー。message はデバッグ用途（ユーザー向け文言はシェルが持つ — ADR 0002）。
#[derive(Debug, uniffi::Error)]
pub enum ToolkitError {
    /// JSON のパースに失敗（SAMPLE_DTO_V2 として読めない）。
    InvalidJson { message: String },
    /// DTO → domain の変換に失敗（スキーマバージョン不一致・未知の kind 等）。
    Conversion { message: String },
}

impl std::fmt::Display for ToolkitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { message } => write!(f, "invalid json: {message}"),
            Self::Conversion { message } => write!(f, "conversion failed: {message}"),
        }
    }
}

impl std::error::Error for ToolkitError {}

/// コアのバージョン文字列。FFI 疎通確認の最小関数。
#[uniffi::export]
pub fn toolkit_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// SAMPLE_DTO_V2 の試合 JSON を取り込み、SummaryProjection の結果を JSON 文字列で返す。
///
/// 内部 ID は連番から決定的に生成する。コアは ID を生成しない設計（設計不変条件）のため
/// 供給責務はシェル側にあり、この関数の出力は内部 ID を含まないので連番で足りる。
#[uniffi::export]
pub fn summarize_sample_match(sample_json: String) -> Result<String, ToolkitError> {
    let dto: SampleMatchDtoV2 =
        serde_json::from_str(&sample_json).map_err(|error| ToolkitError::InvalidJson {
            message: error.to_string(),
        })?;

    let mut counter: u128 = 0;
    let converted = convert("ffi", &dto, None, || {
        counter += 1;
        Uuid::from_u128(counter)
    })
    .map_err(|error| ToolkitError::Conversion {
        message: format!("{error:?}"),
    })?;

    let summary = SummaryProjection::build(&converted.r#match, &converted.facts);
    let response = SummaryResponse::from_projection(&summary, &converted);

    // 直列化は自前型のみなので失敗しない（Map キーも文字列のみ）
    Ok(serde_json::to_string_pretty(&response).expect("SummaryResponse は常に直列化可能"))
}

/// FFI 境界の出力形。内部 UUID は漏らさず、チーム名・選手名へ解決して返す。
/// キーはコーパス（SAMPLE_DTO_V2 / golden）と同じ camelCase。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryResponse {
    title: Option<String>,
    home_team: TeamLine,
    away_team: TeamLine,
    home_score: i64,
    away_score: i64,
    player_stats: Vec<PlayerLine>,
    fact_count: usize,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TeamLine {
    name: String,
    goals: i64,
    shot_misses: i64,
    shot_attempts: i64,
    scoring_rate: Option<f64>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerLine {
    name: String,
    jersey_number: Option<i64>,
    goals: i64,
    shot_misses: i64,
}

impl SummaryResponse {
    fn from_projection(
        summary: &SummaryProjection,
        converted: &handball_toolkit::sample_dto::SampleMatchConversionResult,
    ) -> SummaryResponse {
        let team_line = |line: &handball_toolkit::projection::TeamSummaryLine| {
            let name = if line.team_id == converted.home_team.id {
                converted.home_team.name.clone()
            } else {
                converted.away_team.name.clone()
            };
            TeamLine {
                name,
                goals: line.goals,
                shot_misses: line.shot_misses,
                shot_attempts: line.shot_attempts(),
                scoring_rate: line.scoring_rate(),
            }
        };

        // 表示用の整列（goals 降順 → 名前昇順）。コアの決定的ソート（uuid 昇順 —
        // 保存セマンティクス 9）はここでは同期 ID のため意味を持たない
        let mut player_stats: Vec<PlayerLine> = summary
            .player_stats
            .iter()
            .map(|line| {
                let player = converted
                    .players
                    .iter()
                    .find(|player| player.id == line.player_id);
                PlayerLine {
                    name: player.map_or_else(String::new, |p| p.name.clone()),
                    jersey_number: player.and_then(|p| p.jersey_number),
                    goals: line.goals,
                    shot_misses: line.shot_misses,
                }
            })
            .collect();
        player_stats.sort_by(|a, b| b.goals.cmp(&a.goals).then_with(|| a.name.cmp(&b.name)));

        SummaryResponse {
            title: converted.r#match.title.clone(),
            home_team: team_line(&summary.home_team),
            away_team: team_line(&summary.away_team),
            home_score: summary.home_score,
            away_score: summary.away_score,
            player_stats,
            fact_count: converted.facts.len(),
        }
    }
}
