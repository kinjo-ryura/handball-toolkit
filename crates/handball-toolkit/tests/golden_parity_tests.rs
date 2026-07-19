//! ゴールデンコーパスによるパリティ検証（ADR 0003）。
//!
//! `tests/golden/inputs/` のコーパス JSON を sample_dto で取り込み、Rust 実装の
//! 5 系統（resolver / timeline / summary / scoreProgression / liveSamples）の出力を
//! Swift オラクルの期待値 `tests/golden/expected/` と突き合わせる。
//!
//! - 比較は JSON 構造比較（f64 は parse 後の完全一致 = bit-exact — ADR 0003 §5）
//! - ID はコーパス由来キーへ正規化（factID / teamKey / playerKey — ADR 0003 §3）。
//!   `summary.playerStats` は playerKey 昇順に整列して比較する
//! - `liveSamples` は期待値に記録された video 位置を再生して build する
//!   （サンプリング規則は dump ツール側にのみ存在 — `tests/golden/README.md`）
//! - `tests/golden/local/`（gitignore 済み）が存在すればローカル `.timer` コーパスも検証する

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use handball_toolkit::clock::VideoClock;
use handball_toolkit::configuration::PhaseKind;
use handball_toolkit::entities::Match;
use handball_toolkit::facts::StoppageKind;
use handball_toolkit::ids::{PlayerId, TeamId};
use handball_toolkit::projection::{
    LiveMatchProjection, MatchTimerState, ScoreProgressionProjection, SegmentResolver,
    SummaryProjection, TimeSegmentKind, TimelineProjection,
};
use handball_toolkit::sample_dto::{SampleMatchDtoV2, convert};
use serde::Deserialize;
use uuid::Uuid;

// ── golden 形式（dump ツールの GoldenDump.swift とミラー） ──

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Golden {
    resolver: GoldenResolver,
    timeline: Vec<GoldenTimelineEntry>,
    summary: GoldenSummary,
    #[serde(default)]
    score_progression: Option<GoldenScoreProgression>,
    live_samples: Vec<GoldenLiveSample>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenResolver {
    phases: Vec<GoldenPhase>,
    segments: Vec<GoldenSegment>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenPhase {
    #[serde(rename = "factID")]
    fact_id: String,
    kind: String,
    #[serde(default)]
    match_elapsed_start: Option<f64>,
    #[serde(default)]
    match_elapsed_end: Option<f64>,
    #[serde(default)]
    video_elapsed_start: Option<f64>,
    #[serde(default)]
    video_elapsed_end: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenSegment {
    kind: String,
    #[serde(default)]
    phase_kind: Option<String>,
    match_elapsed_start: f64,
    #[serde(default)]
    match_elapsed_end: Option<f64>,
    #[serde(default)]
    video_elapsed_start: Option<f64>,
    #[serde(default)]
    video_elapsed_end: Option<f64>,
    #[serde(default, rename = "startFactID")]
    start_fact_id: Option<String>,
    #[serde(default, rename = "endFactID")]
    end_fact_id: Option<String>,
    #[serde(default)]
    stoppage_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenTimelineEntry {
    #[serde(rename = "factID")]
    fact_id: String,
    #[serde(default)]
    resolved_match_clock: Option<f64>,
    #[serde(default)]
    resolved_video_clock: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenSummary {
    home_score: i64,
    away_score: i64,
    home_team: GoldenTeamLine,
    away_team: GoldenTeamLine,
    player_stats: Vec<GoldenPlayerStat>,
    phase_summaries: Vec<GoldenPhaseSummary>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenTeamLine {
    team_key: String,
    goals: i64,
    shot_misses: i64,
    shot_attempts: i64,
    #[serde(default)]
    scoring_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenPlayerStat {
    player_key: String,
    goals: i64,
    shot_misses: i64,
    shot_attempts: i64,
    #[serde(default)]
    scoring_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenPhaseSummary {
    #[serde(rename = "phaseFactID")]
    phase_fact_id: String,
    kind: String,
    #[serde(default)]
    regular_index: Option<i64>,
    home_goals: i64,
    home_shot_misses: i64,
    away_goals: i64,
    away_shot_misses: i64,
    home_attempts: i64,
    away_attempts: i64,
    #[serde(default)]
    home_rate: Option<f64>,
    #[serde(default)]
    away_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenScoreProgression {
    points: Vec<GoldenScorePoint>,
    phase_spans: Vec<GoldenPhaseSpan>,
    total_seconds: f64,
    max_abs_diff: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenScorePoint {
    cumulative_seconds: f64,
    home_score: i64,
    away_score: i64,
    diff: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenPhaseSpan {
    #[serde(rename = "phaseFactID")]
    phase_fact_id: String,
    regular_index: i64,
    start_seconds: f64,
    end_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenLiveSample {
    #[serde(default)]
    video_elapsed_seconds: Option<f64>,
    #[serde(default)]
    current_phase_kind: Option<String>,
    #[serde(default)]
    current_phase_index: Option<i64>,
    timer_state: String,
    #[serde(default)]
    current_match_clock_seconds: Option<f64>,
    available_actions: GoldenAvailableActions,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenAvailableActions {
    can_record_goal: bool,
    can_record_shot_missed: bool,
    can_record_free_note: bool,
    can_start_timeout: bool,
    can_resume: bool,
    can_start_next_phase: bool,
}

// ── Rust 実装の出力を golden 形式へ正規化 ──

fn phase_kind_raw(kind: PhaseKind) -> String {
    match kind {
        PhaseKind::Regular => "regular".to_owned(),
        PhaseKind::Shootout => "shootout".to_owned(),
    }
}

fn stoppage_kind_raw(kind: StoppageKind) -> String {
    match kind {
        StoppageKind::Timeout => "timeout".to_owned(),
        StoppageKind::Pause => "pause".to_owned(),
    }
}

fn segment_kind_raw(kind: TimeSegmentKind) -> String {
    match kind {
        TimeSegmentKind::Running => "running".to_owned(),
        TimeSegmentKind::Stopped => "stopped".to_owned(),
    }
}

fn timer_state_raw(state: MatchTimerState) -> String {
    match state {
        MatchTimerState::BeforeMatch => "beforeMatch".to_owned(),
        MatchTimerState::Playing => "playing".to_owned(),
        MatchTimerState::Timeout => "timeout".to_owned(),
        MatchTimerState::Paused => "paused".to_owned(),
        MatchTimerState::BetweenPhases => "betweenPhases".to_owned(),
        MatchTimerState::Ended => "ended".to_owned(),
    }
}

/// UUID → コーパスキー表記（`Uuid::to_string()` は小文字 = dump ツール側の lowercased と一致）。
fn fact_key(id: Uuid) -> String {
    id.to_string()
}

struct Normalizer {
    team_key_by_id: BTreeMap<TeamId, String>,
    player_key_by_id: BTreeMap<PlayerId, String>,
}

impl Normalizer {
    fn team_key(&self, id: TeamId) -> String {
        self.team_key_by_id
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("unknown:{}", fact_key(id.0)))
    }

    fn player_key(&self, id: PlayerId) -> String {
        self.player_key_by_id
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("unknown:{}", fact_key(id.0)))
    }
}

fn golden_resolver(resolver: &SegmentResolver) -> GoldenResolver {
    GoldenResolver {
        phases: resolver
            .phases
            .iter()
            .map(|phase| GoldenPhase {
                fact_id: fact_key(phase.fact_id.0),
                kind: phase_kind_raw(phase.kind),
                match_elapsed_start: phase.match_elapsed_start,
                match_elapsed_end: phase.match_elapsed_end,
                video_elapsed_start: phase.video_elapsed_start,
                video_elapsed_end: phase.video_elapsed_end,
            })
            .collect(),
        segments: resolver
            .segments
            .iter()
            .map(|segment| GoldenSegment {
                kind: segment_kind_raw(segment.kind),
                phase_kind: segment.phase_kind.map(phase_kind_raw),
                match_elapsed_start: segment.match_elapsed_start,
                match_elapsed_end: segment.match_elapsed_end,
                video_elapsed_start: segment.video_elapsed_start,
                video_elapsed_end: segment.video_elapsed_end,
                start_fact_id: segment.start_fact_id.map(|id| fact_key(id.0)),
                end_fact_id: segment.end_fact_id.map(|id| fact_key(id.0)),
                stoppage_kind: segment.stoppage_kind.map(stoppage_kind_raw),
            })
            .collect(),
    }
}

fn golden_timeline(timeline: &TimelineProjection) -> Vec<GoldenTimelineEntry> {
    timeline
        .resolved_facts
        .iter()
        .map(|resolved| GoldenTimelineEntry {
            fact_id: fact_key(resolved.fact.id.0),
            resolved_match_clock: resolved.resolved_match_clock.map(|c| c.elapsed_seconds),
            resolved_video_clock: resolved.resolved_video_clock.map(|c| c.elapsed_seconds),
        })
        .collect()
}

fn golden_summary(summary: &SummaryProjection, normalizer: &Normalizer) -> GoldenSummary {
    let mut player_stats: Vec<GoldenPlayerStat> = summary
        .player_stats
        .iter()
        .map(|line| GoldenPlayerStat {
            player_key: normalizer.player_key(line.player_id),
            goals: line.goals,
            shot_misses: line.shot_misses,
            shot_attempts: line.shot_attempts(),
            scoring_rate: line.scoring_rate(),
        })
        .collect();
    player_stats.sort_by(|a, b| a.player_key.cmp(&b.player_key));

    GoldenSummary {
        home_score: summary.home_score,
        away_score: summary.away_score,
        home_team: GoldenTeamLine {
            team_key: normalizer.team_key(summary.home_team.team_id),
            goals: summary.home_team.goals,
            shot_misses: summary.home_team.shot_misses,
            shot_attempts: summary.home_team.shot_attempts(),
            scoring_rate: summary.home_team.scoring_rate(),
        },
        away_team: GoldenTeamLine {
            team_key: normalizer.team_key(summary.away_team.team_id),
            goals: summary.away_team.goals,
            shot_misses: summary.away_team.shot_misses,
            shot_attempts: summary.away_team.shot_attempts(),
            scoring_rate: summary.away_team.scoring_rate(),
        },
        player_stats,
        phase_summaries: summary
            .phase_summaries
            .iter()
            .map(|line| GoldenPhaseSummary {
                phase_fact_id: fact_key(line.phase_fact_id.0),
                kind: phase_kind_raw(line.kind),
                regular_index: line.regular_index.map(|index| index as i64),
                home_goals: line.home_goals,
                home_shot_misses: line.home_shot_misses,
                away_goals: line.away_goals,
                away_shot_misses: line.away_shot_misses,
                home_attempts: line.home_attempts(),
                away_attempts: line.away_attempts(),
                home_rate: line.home_rate(),
                away_rate: line.away_rate(),
            })
            .collect(),
    }
}

fn golden_score_progression(progression: &ScoreProgressionProjection) -> GoldenScoreProgression {
    GoldenScoreProgression {
        points: progression
            .points
            .iter()
            .map(|point| GoldenScorePoint {
                cumulative_seconds: point.cumulative_seconds,
                home_score: point.home_score,
                away_score: point.away_score,
                diff: point.diff(),
            })
            .collect(),
        phase_spans: progression
            .phase_spans
            .iter()
            .map(|span| GoldenPhaseSpan {
                phase_fact_id: fact_key(span.phase_fact_id.0),
                regular_index: span.regular_index as i64,
                start_seconds: span.start_seconds,
                end_seconds: span.end_seconds,
            })
            .collect(),
        total_seconds: progression.total_seconds,
        max_abs_diff: progression.max_abs_diff,
    }
}

/// 期待値に記録された video 位置を再生して liveSamples を構築する。
fn golden_live_samples(
    match_: &Match,
    timeline: &TimelineProjection,
    expected: &[GoldenLiveSample],
) -> Vec<GoldenLiveSample> {
    expected
        .iter()
        .map(|sample| {
            let projection = LiveMatchProjection::build_video_mode(
                match_,
                timeline,
                sample
                    .video_elapsed_seconds
                    .map(|elapsed_seconds| VideoClock { elapsed_seconds }),
            );
            GoldenLiveSample {
                video_elapsed_seconds: sample.video_elapsed_seconds,
                current_phase_kind: projection.current_phase_kind.map(phase_kind_raw),
                current_phase_index: projection.current_phase_index.map(|index| index as i64),
                timer_state: timer_state_raw(projection.timer_state),
                current_match_clock_seconds: projection
                    .current_match_clock
                    .map(|clock| clock.elapsed_seconds),
                available_actions: GoldenAvailableActions {
                    can_record_goal: projection.available_actions.can_record_goal,
                    can_record_shot_missed: projection.available_actions.can_record_shot_missed,
                    can_record_free_note: projection.available_actions.can_record_free_note,
                    can_start_timeout: projection.available_actions.can_start_timeout,
                    can_resume: projection.available_actions.can_resume,
                    can_start_next_phase: projection.available_actions.can_start_next_phase,
                },
            }
        })
        .collect()
}

// ── 照合 ──

fn assert_slice_eq<T: PartialEq + std::fmt::Debug>(
    slug: &str,
    section: &str,
    actual: &[T],
    expected: &[T],
) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{slug}: {section} の件数が不一致（actual {} / expected {}）",
        actual.len(),
        expected.len()
    );
    for (index, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a, e, "{slug}: {section}[{index}] が不一致");
    }
}

fn verify_corpus_file(input_path: &Path, expected_path: &Path) {
    let slug = input_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("<unknown>")
        .to_owned();

    let input_json = fs::read_to_string(input_path)
        .unwrap_or_else(|error| panic!("{slug}: 入力の読込に失敗: {error}"));
    let dto: SampleMatchDtoV2 = serde_json::from_str(&input_json)
        .unwrap_or_else(|error| panic!("{slug}: 入力の decode に失敗: {error}"));

    let expected_json = fs::read_to_string(expected_path)
        .unwrap_or_else(|error| panic!("{slug}: 期待値の読込に失敗: {error}"));
    let expected: Golden = serde_json::from_str(&expected_json)
        .unwrap_or_else(|error| panic!("{slug}: 期待値の decode に失敗: {error}"));

    // ID は正規化で消えるため決定的な連番 UUID で供給する。
    let mut counter: u128 = 0;
    let conversion = convert(&slug, &dto, None, || {
        counter += 1;
        Uuid::from_u128(counter)
    })
    .unwrap_or_else(|error| panic!("{slug}: convert に失敗: {error:?}"));

    let normalizer = Normalizer {
        team_key_by_id: conversion
            .teams_by_key
            .iter()
            .map(|(key, id)| (*id, key.clone()))
            .collect(),
        player_key_by_id: conversion
            .players_by_key
            .iter()
            .map(|(key, id)| (*id, key.clone()))
            .collect(),
    };

    let match_ = &conversion.r#match;
    let facts = &conversion.facts;

    let resolver = SegmentResolver::build(facts);
    let timeline = TimelineProjection::build(match_, facts);
    let summary = SummaryProjection::build_with_timeline(match_, &timeline);
    let progression = ScoreProgressionProjection::build_with_timeline(match_, &timeline);

    let actual_resolver = golden_resolver(&resolver);
    assert_slice_eq(
        &slug,
        "resolver.phases",
        &actual_resolver.phases,
        &expected.resolver.phases,
    );
    assert_slice_eq(
        &slug,
        "resolver.segments",
        &actual_resolver.segments,
        &expected.resolver.segments,
    );
    assert_slice_eq(
        &slug,
        "timeline",
        &golden_timeline(&timeline),
        &expected.timeline,
    );
    assert_eq!(
        golden_summary(&summary, &normalizer),
        expected.summary,
        "{slug}: summary が不一致"
    );
    assert_eq!(
        progression.as_ref().map(golden_score_progression),
        expected.score_progression,
        "{slug}: scoreProgression が不一致"
    );
    assert_slice_eq(
        &slug,
        "liveSamples",
        &golden_live_samples(match_, &timeline, &expected.live_samples),
        &expected.live_samples,
    );
}

/// `inputs/` 配下の JSON を列挙し、対応する `expected/` と突き合わせる。
/// 検証したファイル数を返す。
fn verify_corpus_dir(golden_dir: &Path) -> usize {
    let inputs_dir = golden_dir.join("inputs");
    let mut input_paths: Vec<PathBuf> = Vec::new();
    for group in fs::read_dir(&inputs_dir)
        .unwrap_or_else(|error| panic!("{} の列挙に失敗: {error}", inputs_dir.display()))
    {
        let group = group.unwrap().path();
        if !group.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&group).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                input_paths.push(path);
            }
        }
    }
    input_paths.sort();

    for input_path in &input_paths {
        let relative = input_path.strip_prefix(&inputs_dir).unwrap();
        let expected_path = golden_dir.join("expected").join(relative);
        verify_corpus_file(input_path, &expected_path);
    }
    input_paths.len()
}

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

// ── テスト ──

/// 公開コーパス 8 件（matches 2 + highlights 6）の 5 系統 bit-exact 照合。
#[test]
fn public_corpus_matches_oracle() {
    let verified = verify_corpus_dir(&golden_root());
    assert_eq!(verified, 8, "公開コーパスは 8 件のはず（列挙漏れ検知）");
}

/// ローカル `.timer` コーパス（tests/golden/local/ — gitignore 済み）の照合。
/// ディレクトリが無い環境（standalone clone / CI）ではスキップする。
#[test]
fn local_timer_corpus_matches_oracle_if_present() {
    let local_dir = golden_root().join("local");
    if !local_dir.join("inputs").is_dir() {
        eprintln!("tests/golden/local/ が無いためスキップ（ローカル実行専用 — ADR 0003 §1）");
        return;
    }
    let verified = verify_corpus_dir(&local_dir);
    assert!(verified > 0, "local/inputs/ に JSON がありません");
    eprintln!("ローカルコーパス {verified} 件を照合");
}
