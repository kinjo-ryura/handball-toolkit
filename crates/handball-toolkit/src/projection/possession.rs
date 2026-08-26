//! ポゼッション区間の projection（handball-project#217 / #220）。移植元なし — Rust コアで新規に足した。
//!
//! `PossessionFact` が持つのは**始まり**（そのチームのプレーが動き出した瞬間）と、供給源が
//! 出せた場合の**任意の終わり**だけ。区間そのものは記録せず、ここで fact 列から導出する
//! （HandballRecorder `CONTEXT.md`「ポゼッション (Possession)」）。
//!
//! 終わりの決め方は **`明示 end` → `区間内の同チーム goal` → `次のポゼッション開始 / phase end`**
//! の順で、どれも最後の「次の開始 / phase end」で**クランプする**（#220）。goal を見るのは、
//! それが今使っている上界（次の開始）より**締まった上界**だから — 新しいポゼッション開始の定義
//! （スローオフが実行された瞬間）では得点は必ずその区間の内側に入り、実測でも goal → 次の変化点は
//! 中央値 4〜6 秒ある。goal の時刻が「遅い端の 1 点」（放送により最大 +25s 遅れる。#153）で
//! あることは承知のうえで採っている — クランプがあるので、goal が遅れた場合でも**今日の値
//! （次の開始 / phase end）に戻るだけで悪くならない**。

use serde::{Deserialize, Serialize};

use crate::clock::FactAnchor;
use crate::entities::Match;
use crate::facts::{MatchFact, MatchFactPayload, PlayEventKind};
use crate::ids::{FactId, TeamId};

use super::timeline::{TimelineProjection, resolve_anchor_clocks};

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
    /// 区間の終わり（累積 matchClock 秒）。**明示 end → 区間内の同チーム goal →
    /// 同じ phase の次のポゼッション開始 / その phase の end** の順に決め、
    /// いずれも「次の開始 / phase end」でクランプする（handball-project#220）。
    ///
    /// **どれを使ったかはここに残さない。** #148 の採点（`score_possessions.py`）は v2 JSON の
    /// `facts[]` を直接読んで projection を通らないので由来フィールドは届かず、必要な区別
    /// （その fact に終わりが書かれているか）は `PossessionFact::end_anchor` の有無として
    /// 契約側に既にある。
    pub match_elapsed_end: f64,
    /// 区間の始まり（videoClock 秒）。動画に紐付いていない fact では None。
    pub video_elapsed_start: Option<f64>,
    /// 区間の終わり（videoClock 秒）。`match_elapsed_end` を決めたのと**同じ出所**の video を採る
    /// （明示 end ならその end anchor、goal ならその goal fact、クランプが効いたなら次の開始 /
    /// phase end）。その出所が video を解決できないときは None。
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
            /// 明示 end（`PossessionFact::end_anchor`）を解決したもの。matchClock を解決できない
            /// end は「書かれていない」のと同じ扱いにする — 順序も phase も決まらないので
            /// 上界として使えない（start が解けない fact を `unresolved` へ落とすのと同じ理由だが、
            /// **fact ごと落としはしない**。始まりは分かっているので区間は作れる）。
            explicit_end: Option<(f64, Option<f64>)>,
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
            // **区間は phase をまたがない。** 終わりの最後の拠り所が「次のポゼッション開始、
            // 無ければ phase の end」なので、phase の外に置かれた fact には上界が定義できない。
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
                explicit_end: possession
                    .end_anchor
                    .and_then(|anchor| resolve_end_anchor(anchor, timeline)),
            });
        }

        // matchClock 昇順。`sort_by` は stable なので、同時刻は fact 列の出現順が残る
        // （`recorded_at` の tie-break は timeline 側で済んでいる）。
        rows.sort_by(|a, b| a.match_start.total_cmp(&b.match_start));

        let goals = collect_goals(timeline);

        let mut segments: Vec<PossessionSegment> = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            // 上界 = 次の区間が**同じ phase**にあるときだけその開始、無ければ自分の phase の end。
            // phase が変われば（= 次の phase の 1 件目）自分の phase の end で閉じる。
            let next_in_phase = rows
                .get(index + 1)
                .filter(|next| next.phase_fact_id == row.phase_fact_id);
            let (bound_match, bound_video) = match next_in_phase {
                Some(next) => (next.match_start, next.video_start),
                None => (row.phase_match_end, row.phase_video_end),
            };

            // 明示 end → 区間内の同チーム goal → 上界そのもの、の順に締める。
            let candidate = row
                .explicit_end
                .or_else(|| first_goal_in(&goals, row.team_id, row.match_start, bound_match));
            // **どの経路もクランプする**（#220）。明示 end は上界より後ろを指しうるし（供給源が
            // 112〜202 件 / 試合を書き、順序の逆転は故障ではなく常態 — validation ではなくここで
            // 吸収する）、goal も上界と同時刻になりうる。クランプが入るので区間の重なりは
            // 構造的に起こりえない。
            let (match_end, video_end) = match candidate {
                Some((match_end, video_end)) if match_end <= bound_match => (match_end, video_end),
                _ => (bound_match, bound_video),
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

/// 明示 end anchor を (matchClock 秒, videoClock 秒) へ解決する。
///
/// `ResolvedFact` が持つのは代表 anchor（= 始まり）だけなので、end は resolver から解き直す。
/// matchClock を解決できなければ `None` = 「end が書かれていない」と同じ扱い（区間の順序は
/// matchClock で決まるので、上界として使えない）。
fn resolve_end_anchor(
    anchor: FactAnchor,
    timeline: &TimelineProjection,
) -> Option<(f64, Option<f64>)> {
    let (mc, vc) = resolve_anchor_clocks(anchor, &timeline.resolver);
    Some((mc?.elapsed_seconds, vc.map(|vc| vc.elapsed_seconds)))
}

/// goal fact を (matchClock 秒, videoClock 秒, teamId) で取り出し matchClock 昇順に並べる。
///
/// `shotMissed` は拾わない — ポゼッションを終わらせるとは限らない（リバウンドで同じチームが
/// 続く）。得点だけが「そこで攻撃が終わった」を確実に語る。team が付いていない goal も落とす
/// （どちらのポゼッションを閉じるか決められない）。
fn collect_goals(timeline: &TimelineProjection) -> Vec<(f64, Option<f64>, TeamId)> {
    let mut goals: Vec<(f64, Option<f64>, TeamId)> = timeline
        .resolved_facts
        .iter()
        .filter_map(|resolved| {
            let MatchFactPayload::Play(play) = &resolved.fact.payload else {
                return None;
            };
            if play.kind != PlayEventKind::Goal {
                return None;
            }
            Some((
                resolved.resolved_match_clock?.elapsed_seconds,
                resolved.resolved_video_clock.map(|vc| vc.elapsed_seconds),
                play.team_id?,
            ))
        })
        .collect();
    goals.sort_by(|a, b| a.0.total_cmp(&b.0));
    goals
}

/// `(start, bound]` の中で最初に現れる `team_id` の goal。
///
/// **最初のもの**を採るのが一番締まった上界になる。2 件以上入るのは後続のポゼッション開始を
/// 取りこぼしたときで（供給源のカバレッジは 44〜81%）、そのとき実際にこの区間を終わらせたのは
/// 1 件目の得点。start ちょうどの goal を含めないのは 0 長の区間を作らないため。
fn first_goal_in(
    goals: &[(f64, Option<f64>, TeamId)],
    team_id: TeamId,
    start: f64,
    bound: f64,
) -> Option<(f64, Option<f64>)> {
    let from = goals.partition_point(|(seconds, _, _)| *seconds <= start);
    goals[from..]
        .iter()
        .take_while(|(seconds, _, _)| *seconds <= bound)
        .find(|(_, _, goal_team)| *goal_team == team_id)
        .map(|(seconds, video, _)| (*seconds, *video))
}
