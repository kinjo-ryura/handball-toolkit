//! ポゼッション区間の projection（handball-project#217）。移植元なし — Rust コアで新規に足した。
//!
//! `PossessionFact` は**点**（ボールを保持した瞬間）だけを記録する。区間は記録せず、
//! ここで fact 列から導出する（HandballRecorder `CONTEXT.md`「ポゼッション (Possession)」）。

use serde::{Deserialize, Serialize};

use crate::entities::Match;
use crate::facts::{MatchFact, MatchFactPayload};
use crate::ids::{FactId, TeamId};

use super::timeline::TimelineProjection;

/// 1 チームがボールを保持していた区間。**`PossessionFact` 1 件につき 1 つ**作る。
///
/// fact と 1 対 1 なのは、シェルが区間を選んで**その fact を編集する**ため（時刻を直す /
/// 消す）。「同じチームが連続したら 1 ポゼッション」という数え方は区間をまとめるのではなく
/// `is_redundant` で表す — まとめてしまうと 2 件目の fact に触る手段が消える。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PossessionSegment {
    /// この区間を宣言した `PossessionFact` の id。シェルの選択・編集の対象。
    pub fact_id: FactId,
    /// この区間でボールを保持していたチーム。
    pub team_id: TeamId,
    /// この区間が属する phase（`PhaseStart` fact の id）。区間は phase をまたがない。
    pub phase_fact_id: FactId,
    /// 区間の始まり（累積 matchClock 秒）。宣言した fact の時刻そのもの。
    pub match_elapsed_start: f64,
    /// 区間の終わり（累積 matchClock 秒）。**同じ phase の次のポゼッション開始、
    /// 無ければその phase の end**。
    pub match_elapsed_end: f64,
    /// 区間の始まり（videoClock 秒）。動画に紐付いていない fact では None。
    pub video_elapsed_start: Option<f64>,
    /// 区間の終わり（videoClock 秒）。次のポゼッション開始 / phase end のどちらも
    /// video を解決できないときは None。
    pub video_elapsed_end: Option<f64>,
    /// **同じ phase の直前の区間と同じチーム** = 冗長な宣言。区間としては独立して存在するが、
    /// ポゼッション数には数えない（`CONTEXT.md`「数える単位は fact の件数ではなく
    /// チームが切り替わった回数」）。取りこぼしと違ってこちらは実害が無い代わりに、
    /// 検出器の出力を目で確かめるときは誤検知の候補になる。
    pub is_redundant: bool,
}

impl PossessionSegment {
    /// 区間の長さ（matchClock 秒）。停止区間中は matchClock が進まないので、
    /// これは**実際にプレーしていた秒数**であって動画上の経過ではない。
    pub fn match_elapsed_duration(&self) -> f64 {
        (self.match_elapsed_end - self.match_elapsed_start).max(0.0)
    }
}

/// ポゼッション区間の一覧と、ドメイン定義どおりのポゼッション数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PossessionProjection {
    /// matchClock 昇順。同時刻は fact 列の出現順を保つ。
    pub segments: Vec<PossessionSegment>,
    /// **チームが切り替わった回数**（= `is_redundant` でない区間の数）。
    /// fact の件数ではないので `segments.len()` とは一致しないことがある。
    pub possession_count: usize,
    /// 区間にできなかった `PossessionFact`。matchClock を解決できないか、どの phase にも
    /// 属さない（phase 開始前 / phase と phase の間）もの。**黙って捨てない** —
    /// 取り込んだ件数と一覧の件数が合わない理由をシェルが説明できるようにする。
    pub unresolved_fact_ids: Vec<FactId>,
}

impl PossessionProjection {
    /// facts から timeline を構築して導出する convenience。timeline を既に持つ経路は
    /// `build_with_timeline` を使い resolver を二度作らないこと。
    pub fn build(match_: &Match, facts: &[MatchFact]) -> PossessionProjection {
        Self::build_with_timeline(match_, &TimelineProjection::build(match_, facts))
    }

    /// timeline（resolver）からポゼッション区間を導出する。
    ///
    /// `match_` は未使用だが API 対称性（facts 版 / timeline 版の 2 系統 — ADR 0001 関数目録）の
    /// ため引数に保持する（`TimelineProjection::build` と同じ扱い）。
    pub fn build_with_timeline(
        _match_: &Match,
        timeline: &TimelineProjection,
    ) -> PossessionProjection {
        let resolver = &timeline.resolver;

        struct Row {
            fact_id: FactId,
            team_id: TeamId,
            phase_fact_id: FactId,
            phase_match_end: f64,
            phase_video_end: Option<f64>,
            match_start: f64,
            video_start: Option<f64>,
        }

        let mut rows: Vec<Row> = Vec::new();
        let mut unresolved: Vec<FactId> = Vec::new();

        for resolved in &timeline.resolved_facts {
            let MatchFactPayload::Possession(possession) = &resolved.fact.payload else {
                continue;
            };
            // matchClock が解けない fact は区間にできない（順序も phase も決まらない）。
            let Some(mc) = resolved.resolved_match_clock else {
                unresolved.push(resolved.fact.id);
                continue;
            };
            // **区間は phase をまたがない。** 終わりは「次のポゼッション開始、無ければ phase の end」
            // なので、phase の外に置かれた fact には終わりが定義できない。
            let Some(phase) = resolver.phase_for_match_elapsed(mc.elapsed_seconds) else {
                unresolved.push(resolved.fact.id);
                continue;
            };
            let Some(phase_match_end) = phase.match_elapsed_end else {
                unresolved.push(resolved.fact.id);
                continue;
            };
            rows.push(Row {
                fact_id: resolved.fact.id,
                team_id: possession.team_id,
                phase_fact_id: phase.fact_id,
                phase_match_end,
                phase_video_end: phase.video_elapsed_end,
                match_start: mc.elapsed_seconds,
                video_start: resolved.resolved_video_clock.map(|vc| vc.elapsed_seconds),
            });
        }

        // matchClock 昇順。`sort_by` は stable なので、同時刻は fact 列の出現順が残る
        // （`recorded_at` の tie-break は timeline 側で済んでいる）。
        rows.sort_by(|a, b| a.match_start.total_cmp(&b.match_start));

        let mut segments: Vec<PossessionSegment> = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            // 次の区間が**同じ phase**にあるときだけ、その開始が終わりになる。
            // phase が変われば（= 次の phase の 1 件目）自分の phase の end で閉じる。
            let next_in_phase = rows
                .get(index + 1)
                .filter(|next| next.phase_fact_id == row.phase_fact_id);
            let (match_end, video_end) = match next_in_phase {
                Some(next) => (next.match_start, next.video_start),
                None => (row.phase_match_end, row.phase_video_end),
            };
            let is_redundant = segments.last().is_some_and(|prev: &PossessionSegment| {
                prev.phase_fact_id == row.phase_fact_id && prev.team_id == row.team_id
            });
            segments.push(PossessionSegment {
                fact_id: row.fact_id,
                team_id: row.team_id,
                phase_fact_id: row.phase_fact_id,
                match_elapsed_start: row.match_start,
                // 終わりが始まりより手前に来ることは無いが、phase end より後ろに置かれた
                // fact（degenerate phase 等）で負にならないよう下限を始まりに揃える。
                match_elapsed_end: match_end.max(row.match_start),
                video_elapsed_start: row.video_start,
                video_elapsed_end: video_end,
                is_redundant,
            });
        }

        let possession_count = segments.iter().filter(|s| !s.is_redundant).count();

        PossessionProjection {
            segments,
            possession_count,
            unresolved_fact_ids: unresolved,
        }
    }

    /// `fact_id` の区間を引く。シェルが選択中の fact から区間を求めるのに使う。
    pub fn segment(&self, fact_id: FactId) -> Option<&PossessionSegment> {
        self.segments.iter().find(|s| s.fact_id == fact_id)
    }
}
