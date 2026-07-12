//! 移植元: `Projection/SummaryProjection.swift`。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::configuration::PhaseKind;
use crate::entities::Match;
use crate::facts::{MatchFact, MatchFactPayload, PlayEventKind};
use crate::ids::{FactId, PlayerId, TeamId};

use super::timeline::TimelineProjection;

/// fact log から得点と goal / shotMissed 集計を導出する読み取り専用 projection。
///
/// score / team / player の集計は **anchor 非依存**（kind / teamID / playerID のみ）で、
/// 全 goal を無条件に数える。一方 phase 別 stats（`phase_summaries`）は resolver 依存
/// （各 fact の matchClock を phase 区間へ逆引きする）のため、`build_with_timeline` でのみ
/// 算出する。`build` では `phase_summaries` は空 — 試合一覧・記録画面など
/// phase 別を必要としない経路で resolver 構築コストを払わないため。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryProjection {
    pub home_score: i64,
    pub away_score: i64,
    pub home_team: TeamSummaryLine,
    pub away_team: TeamSummaryLine,
    pub player_stats: Vec<PlayerStatLine>,
    /// phase ごとの home/away stats。**記録のある phase のみ**、resolver の出現順。
    /// `build_with_timeline` でのみ非空。日本語ラベル（前半/後半/7mTC）は持たず、
    /// UI 層が `kind` + `regular_index` から導出する（PhaseKind の「ラベルは UI 層」決定）。
    pub phase_summaries: Vec<PhaseSummaryLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSummaryLine {
    pub team_id: TeamId,
    pub goals: i64,
    pub shot_misses: i64,
}

impl TeamSummaryLine {
    pub fn shot_attempts(&self) -> i64 {
        self.goals + self.shot_misses
    }

    pub fn scoring_rate(&self) -> Option<f64> {
        rate(self.goals, self.shot_attempts())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStatLine {
    pub player_id: PlayerId,
    pub goals: i64,
    pub shot_misses: i64,
}

impl PlayerStatLine {
    pub fn shot_attempts(&self) -> i64 {
        self.goals + self.shot_misses
    }

    pub fn scoring_rate(&self) -> Option<f64> {
        rate(self.goals, self.shot_attempts())
    }
}

/// phase 1 件分の home/away stats。`phase_fact_id` は PhaseStart の FactID（出現順で stable）。
/// 日本語ラベルは持たない: UI が `kind` + `regular_index` から「前半/後半/延長…/7mTC」を導出する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseSummaryLine {
    pub phase_fact_id: FactId,
    pub kind: PhaseKind,
    /// regular phase の出現順 index（0 始まり）。shootout は None。
    pub regular_index: Option<usize>,
    pub home_goals: i64,
    pub home_shot_misses: i64,
    pub away_goals: i64,
    pub away_shot_misses: i64,
}

impl PhaseSummaryLine {
    pub fn home_attempts(&self) -> i64 {
        self.home_goals + self.home_shot_misses
    }

    pub fn away_attempts(&self) -> i64 {
        self.away_goals + self.away_shot_misses
    }

    pub fn home_rate(&self) -> Option<f64> {
        rate(self.home_goals, self.home_attempts())
    }

    pub fn away_rate(&self) -> Option<f64> {
        rate(self.away_goals, self.away_attempts())
    }
}

fn rate(goals: i64, attempts: i64) -> Option<f64> {
    if attempts > 0 {
        Some(goals as f64 / attempts as f64)
    } else {
        None
    }
}

impl SummaryProjection {
    /// score / team / player の集計のみ（resolver 非依存）。`phase_summaries` は空。
    /// phase 別を必要としない経路（試合一覧・記録画面）で使う。
    pub fn build(match_: &Match, facts: &[MatchFact]) -> SummaryProjection {
        let (home, away, player_stats) = aggregate_core(match_, facts);
        SummaryProjection {
            home_score: home.goals,
            away_score: away.goals,
            home_team: home,
            away_team: away,
            player_stats,
            phase_summaries: Vec::new(),
        }
    }

    /// score / team / player に加え、timeline（resolver）から phase 別 stats を導出する。
    /// 試合サマリ画面で使う。呼び出し側が作った `timeline` の resolver を再利用する（resolver は二度作らない）。
    pub fn build_with_timeline(match_: &Match, timeline: &TimelineProjection) -> SummaryProjection {
        let facts: Vec<MatchFact> = timeline
            .resolved_facts
            .iter()
            .map(|r| r.fact.clone())
            .collect();
        let (home, away, player_stats) = aggregate_core(match_, &facts);
        SummaryProjection {
            home_score: home.goals,
            away_score: away.goals,
            home_team: home,
            away_team: away,
            player_stats,
            phase_summaries: build_phase_summaries(match_, timeline),
        }
    }
}

// ── 集計 ──

fn aggregate_core(
    match_: &Match,
    facts: &[MatchFact],
) -> (TeamSummaryLine, TeamSummaryLine, Vec<PlayerStatLine>) {
    let mut home = TeamSummaryLine {
        team_id: match_.home_team_id,
        goals: 0,
        shot_misses: 0,
    };
    let mut away = TeamSummaryLine {
        team_id: match_.away_team_id,
        goals: 0,
        shot_misses: 0,
    };
    // Swift は Dictionary 集計後に uuidString 昇順で sort。BTreeMap の Uuid Ord は
    // バイト順 = hex 文字列順と同順（ADR 0001）のため、キー順 iterate で同じ決定的順序になる。
    let mut player_counts: BTreeMap<PlayerId, (i64, i64)> = BTreeMap::new();

    for fact in facts {
        let MatchFactPayload::Play(play) = &fact.payload else {
            continue;
        };

        match play.kind {
            PlayEventKind::Goal => {
                if play.team_id == Some(match_.home_team_id) {
                    home.goals += 1;
                } else if play.team_id == Some(match_.away_team_id) {
                    away.goals += 1;
                }
                if let Some(pid) = play.player_id {
                    player_counts.entry(pid).or_insert((0, 0)).0 += 1;
                }
            }
            PlayEventKind::ShotMissed => {
                if play.team_id == Some(match_.home_team_id) {
                    home.shot_misses += 1;
                } else if play.team_id == Some(match_.away_team_id) {
                    away.shot_misses += 1;
                }
                if let Some(pid) = play.player_id {
                    player_counts.entry(pid).or_insert((0, 0)).1 += 1;
                }
            }
            _ => {}
        }
    }

    let player_stats = player_counts
        .into_iter()
        .map(|(player_id, (goals, misses))| PlayerStatLine {
            player_id,
            goals,
            shot_misses: misses,
        })
        .collect();

    (home, away, player_stats)
}

/// `SegmentResolver.phases` の出現順から phase ごとの goal / shotMissed を集計する。
/// matchClock を解決できない goal（動画位置が phase 区間外など）は **黙って除外**するため、
/// ここの phase 別合計は header（`home_score` / `away_score`、全 goal 無条件集計）と
/// まれに一致しないことがある（header >= Σ phase）。正常な試合では R7 がこれを禁止するので、
/// import / migration / 壊れたデータの safety net。記録のある phase のみ返す。
fn build_phase_summaries(match_: &Match, timeline: &TimelineProjection) -> Vec<PhaseSummaryLine> {
    let resolver = &timeline.resolver;
    #[derive(Clone, Copy, Default)]
    struct Tally {
        goals: i64,
        missed: i64,
    }
    let mut per_phase: BTreeMap<FactId, (Tally, Tally)> = BTreeMap::new();

    for resolved in &timeline.resolved_facts {
        let MatchFactPayload::Play(p) = &resolved.fact.payload else {
            continue;
        };
        if p.kind != PlayEventKind::Goal && p.kind != PlayEventKind::ShotMissed {
            continue;
        }
        let Some(mc) = resolved.resolved_match_clock else {
            continue;
        };
        let Some(phase) = resolver.phase_for_match_elapsed(mc.elapsed_seconds) else {
            continue;
        };
        let pair = per_phase.entry(phase.fact_id).or_default();
        let is_home = p.team_id == Some(match_.home_team_id);
        let is_away = p.team_id == Some(match_.away_team_id);
        match p.kind {
            PlayEventKind::Goal => {
                if is_home {
                    pair.0.goals += 1;
                } else if is_away {
                    pair.1.goals += 1;
                }
            }
            PlayEventKind::ShotMissed => {
                if is_home {
                    pair.0.missed += 1;
                } else if is_away {
                    pair.1.missed += 1;
                }
            }
            _ => {}
        }
    }

    let mut result: Vec<PhaseSummaryLine> = Vec::new();
    let mut regular_index: usize = 0;
    for phase in &resolver.phases {
        let idx = match phase.kind {
            PhaseKind::Regular => {
                let current = regular_index;
                regular_index += 1;
                Some(current)
            }
            PhaseKind::Shootout => None,
        };
        let Some(pair) = per_phase.get(&phase.fact_id) else {
            continue;
        };
        result.push(PhaseSummaryLine {
            phase_fact_id: phase.fact_id,
            kind: phase.kind,
            regular_index: idx,
            home_goals: pair.0.goals,
            home_shot_misses: pair.0.missed,
            away_goals: pair.1.goals,
            away_shot_misses: pair.1.missed,
        });
    }
    result
}
