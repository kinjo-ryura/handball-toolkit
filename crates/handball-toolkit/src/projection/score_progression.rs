//! 移植元: `Projection/ScoreProgressionProjection.swift`。

use serde::{Deserialize, Serialize};

use crate::entities::Match;
use crate::facts::{MatchFact, MatchFactPayload, PlayEventKind};
use crate::ids::FactId;

use super::timeline::TimelineProjection;

/// 得点差の時系列遷移を表す読み取り専用 projection（得点差チャート用）。
///
/// `points` は **step-doubling 済み**: 各 goal 時刻に「goal 前」「goal 後」の 2 点を持つ。
/// これは描画ハックではなく、**step function（区間一定で goal の瞬間に
/// 不連続ジャンプする折れ線）の不連続点を 2 つの y 値で明示エンコードしたもの**。
/// これにより view は補間方法を気にせず折れ線でそのまま階段を描け、
/// かつ「実際に描画される系列そのもの」を単体テストできる。
///
/// 各 point は累積スコア（`home_score` / `away_score`）を持ち、点差は `diff`（away − home）で導出する。
/// 横軸の符号（左=ホームリード）・軸スケール・ズーム・時間軸の日本語ラベル（`前半 MM:SS` 等）は
/// view の責務。`phase_spans` はラベルを持たず、view が `regular_index` からラベルを導出する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreProgressionProjection {
    pub points: Vec<ScoreProgressionPoint>,
    /// regular phase の区間（累積 matchClock 秒）。時間軸ラベル / 境界線用。出現順。
    pub phase_spans: Vec<ScoreProgressionPhaseSpan>,
    pub total_seconds: f64,
    /// 横軸スケール用の最大絶対点差（= どちらかの最大リード、最低 1）。
    pub max_abs_diff: i64,
}

/// 得点差チャートの 1 点。step-doubling のため 1 goal につき 2 点（goal 前/後）作られる。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreProgressionPoint {
    pub cumulative_seconds: f64,
    pub home_score: i64,
    pub away_score: i64,
}

impl ScoreProgressionPoint {
    /// away − home。負がホームリード（チャート左）、正がアウェイリード（チャート右）。
    pub fn diff(&self) -> i64 {
        self.away_score - self.home_score
    }
}

/// 時間軸ラベル用の regular phase 区間（累積 matchClock 秒）。ラベルは持たない（view が導出）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreProgressionPhaseSpan {
    pub phase_fact_id: FactId,
    /// regular phase の出現順 index（0 始まり）。
    pub regular_index: usize,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl ScoreProgressionProjection {
    /// facts から timeline を構築して導出する convenience。timeline を既に持つ live 経路は
    /// `build_with_timeline` を使い resolver を二度作らないこと。
    pub fn build(match_: &Match, facts: &[MatchFact]) -> Option<ScoreProgressionProjection> {
        Self::build_with_timeline(match_, &TimelineProjection::build(match_, facts))
    }

    /// timeline（resolver）から得点差遷移を導出する。regular phase が無い / goal が 1 件も無い場合は None。
    /// goal の matchClock を解決できない場合はその goal を除外する（header スコアとまれに不一致）。
    /// shootout goal は degenerate clock（固定累積秒）のため最終 regular phase 終端に重なる（現状維持）。
    pub fn build_with_timeline(
        match_: &Match,
        timeline: &TimelineProjection,
    ) -> Option<ScoreProgressionProjection> {
        let resolver = &timeline.resolver;

        // 時間軸は regular phase の実 matchClock 境界。
        let mut spans: Vec<ScoreProgressionPhaseSpan> = Vec::new();
        let mut regular_index: usize = 0;
        let mut max_end: f64 = 0.0;
        for phase in resolver
            .phases
            .iter()
            .filter(|p| p.kind == crate::configuration::PhaseKind::Regular)
        {
            let (Some(start), Some(end)) = (phase.match_elapsed_start, phase.match_elapsed_end)
            else {
                continue;
            };
            spans.push(ScoreProgressionPhaseSpan {
                phase_fact_id: phase.fact_id,
                regular_index,
                start_seconds: start,
                end_seconds: end,
            });
            regular_index += 1;
            max_end = max_end.max(end);
        }
        if spans.is_empty() {
            return None;
        }
        let total_seconds = max_end.max(1.0);

        // goal のみ拾う（shotMissed は点差に効かない）。home 以外（unknown team 含む）は away 扱い。
        struct GoalRow {
            cumulative_seconds: f64,
            is_home: bool,
        }
        let mut rows: Vec<GoalRow> = Vec::new();
        for resolved in &timeline.resolved_facts {
            let MatchFactPayload::Play(p) = &resolved.fact.payload else {
                continue;
            };
            if p.kind != PlayEventKind::Goal {
                continue;
            }
            let Some(mc) = resolved.resolved_match_clock else {
                continue;
            };
            rows.push(GoalRow {
                cumulative_seconds: mc.elapsed_seconds,
                is_home: p.team_id == Some(match_.home_team_id),
            });
        }
        if rows.is_empty() {
            return None;
        }
        rows.sort_by(|a, b| a.cumulative_seconds.total_cmp(&b.cumulative_seconds));

        // step-doubling: 各 goal 時刻に goal 前 / goal 後の 2 点。先頭 (0,0)・末尾 (totalSeconds,最終) を付加。
        let mut points: Vec<ScoreProgressionPoint> = vec![ScoreProgressionPoint {
            cumulative_seconds: 0.0,
            home_score: 0,
            away_score: 0,
        }];
        let mut home: i64 = 0;
        let mut away: i64 = 0;
        let mut max_abs: i64 = 0;
        for row in &rows {
            points.push(ScoreProgressionPoint {
                cumulative_seconds: row.cumulative_seconds,
                home_score: home,
                away_score: away,
            });
            if row.is_home {
                home += 1;
            } else {
                away += 1;
            }
            points.push(ScoreProgressionPoint {
                cumulative_seconds: row.cumulative_seconds,
                home_score: home,
                away_score: away,
            });
            max_abs = max_abs.max((away - home).abs());
        }
        points.push(ScoreProgressionPoint {
            cumulative_seconds: total_seconds,
            home_score: home,
            away_score: away,
        });

        Some(ScoreProgressionProjection {
            points,
            phase_spans: spans,
            total_seconds,
            max_abs_diff: max_abs.max(1),
        })
    }
}
