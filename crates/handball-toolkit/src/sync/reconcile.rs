//! そろえる — 記録ごとの後勝ちと、利用者に聞くこと（ADR 0007 決定 2〜4）。
//!
//! | 場面 | 結果 |
//! |---|---|
//! | 片方にだけある | 足す |
//! | 両方にあり、中身が同じ | `updated_at` の新しい方の版（中身は同じ） |
//! | 両方にあり、中身が違う | `updated_at` の新しい方。**利用者には聞かない** |
//! | 両方にあり、`updated_at` がまったく同じで中身が違う | 聞く（`SameTime`） |
//! | 両方とも消してある | 新しい方の削除 |
//! | 片方で消してある | 消した時刻が相手の最後の変更より後なら消す。前なら聞く（`DeletedThenChanged`） |
//! | 両方の直しを合わせた試合が規則に合わない | 聞く（`MergedMatchInvalid`）。答えた側の試合を丸ごと採る |
//!
//! **聞いて選ばれた版は `now` で書き直す**。次の同期でどの端末と比べても選ばれた版が新しいので、
//! 同じ結果になる。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::facts::{MatchFact, MatchFactPayload};
use crate::ids::{FactId, MatchId, PlayerId, TeamId};
use crate::persistence_order::persistence_ordered;
use crate::validators::{RosterContext, validate_fact_log, validate_match, validate_match_fact};

use super::compare::{
    fact_content_equal, match_content_equal, player_content_equal, team_content_equal,
};
use super::normalize::{normalized, round_to_millis};
use super::{
    SyncAnswer, SyncFact, SyncMatch, SyncPlayer, SyncQuestion, SyncQuestionKind, SyncRecordRef,
    SyncSide, SyncSnapshot, SyncStamp, SyncTeam,
};

/// [`reconcile`] の結果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SyncReconcileResult {
    /// 利用者に聞くことがある。答えを [`SyncAnswer`] で足して呼び直す。
    /// 答えた後に別の問い（`MergedMatchInvalid`）が出ることがある — 返らなくなるまで繰り返す。
    Questions { questions: Vec<SyncQuestion> },
    /// そろえた中身（端末ごとの値は入れたまま）。両方の端末が [`super::materialize`] して保存する。
    Merged { snapshot: SyncSnapshot },
}

/// 2 台の全記録をそろえる。
///
/// - `local` — 始めた側（この端末）の全記録。消した記録も含む
/// - `remote` — 相手の端末の全記録
/// - `answers` — これまでの問いへの答え。今の問いに当たらないものは無視する
/// - `now` — 聞いて選ばれた版・参照のために戻した記録に付ける時刻（シェルが渡す）
pub fn reconcile(
    local: &SyncSnapshot,
    remote: &SyncSnapshot,
    answers: &[SyncAnswer],
    now: DateTime<Utc>,
) -> SyncReconcileResult {
    // 時刻をミリ秒に丸めてから比べる（`normalize` の doc — 端末との往復で ns の桁がずれる）。
    let (local, remote) = (normalized(local), normalized(remote));
    let (local, remote) = (&local, &remote);
    let now = round_to_millis(now);
    let answers: Answers = answers
        .iter()
        .map(|answer| ((answer.kind, answer.record), answer.keep))
        .collect();
    let ctx = Context {
        answers: &answers,
        now,
    };
    let mut questions: Vec<SyncQuestion> = Vec::new();

    let match_pairs = pairs(&local.matches, &remote.matches, |m| m.match_.id);
    let fact_pairs = pairs(&local.facts, &remote.facts, |f| f.fact.id);
    let team_pairs = pairs(&local.teams, &remote.teams, |t| t.team.id);
    let player_pairs = pairs(&local.players, &remote.players, |p| p.player.id);

    let local_last_changed = last_changed_by_match(local);
    let remote_last_changed = last_changed_by_match(remote);

    let mut matches: BTreeMap<MatchId, SyncMatch> = BTreeMap::new();
    for (id, pair) in &match_pairs {
        let last_changed = (
            local_last_changed.get(id).copied(),
            remote_last_changed.get(id).copied(),
        );
        let record = SyncRecordRef::Match { id: *id };
        match decide(pair, last_changed, record, Some(*id), &ctx) {
            Some(Decision::Take(merged)) => {
                matches.insert(*id, merged);
            }
            Some(Decision::Ask(question)) => questions.push(question),
            None => {}
        }
    }

    let mut facts: BTreeMap<FactId, SyncFact> = BTreeMap::new();
    for (id, pair) in &fact_pairs {
        let match_id = pair.local.or(pair.remote).map(|f| f.match_id);
        let record = SyncRecordRef::Fact { id: *id };
        match decide(pair, (None, None), record, match_id, &ctx) {
            Some(Decision::Take(merged)) => {
                facts.insert(*id, merged);
            }
            Some(Decision::Ask(question)) => questions.push(question),
            None => {}
        }
    }

    let mut teams: BTreeMap<TeamId, SyncTeam> = BTreeMap::new();
    for (id, pair) in &team_pairs {
        let record = SyncRecordRef::Team { id: *id };
        match decide(pair, (None, None), record, None, &ctx) {
            Some(Decision::Take(merged)) => {
                teams.insert(*id, merged);
            }
            Some(Decision::Ask(question)) => questions.push(question),
            None => {}
        }
    }

    let mut players: BTreeMap<PlayerId, SyncPlayer> = BTreeMap::new();
    for (id, pair) in &player_pairs {
        let record = SyncRecordRef::Player { id: *id };
        match decide(pair, (None, None), record, None, &ctx) {
            Some(Decision::Take(merged)) => {
                players.insert(*id, merged);
            }
            Some(Decision::Ask(question)) => questions.push(question),
            None => {}
        }
    }

    if !questions.is_empty() {
        return questions_result(questions);
    }

    revive_referenced(&matches, &facts, &mut teams, &mut players, now);

    // 両方の直しが混ざった試合だけを検証する。片方の版そのままの試合は、その端末で保存するときに
    // 検証を通っている。
    let local_side = SideView::new(&match_pairs, |pair| pair.local, &local.facts);
    let remote_side = SideView::new(&match_pairs, |pair| pair.remote, &remote.facts);
    let mut replacements: Vec<(MatchId, SyncSide)> = Vec::new();
    for (id, merged_match) in &matches {
        if merged_match.stamp.is_deleted() {
            continue;
        }
        let merged_facts = live_facts_of(&facts, *id);
        if local_side.equals(*id, merged_match, &merged_facts)
            || remote_side.equals(*id, merged_match, &merged_facts)
        {
            continue;
        }
        if !merged_match_has_issues(merged_match, &merged_facts, &players) {
            continue;
        }
        let record = SyncRecordRef::Match { id: *id };
        match answers.get(&(SyncQuestionKind::MergedMatchInvalid, record)) {
            Some(&keep) => replacements.push((*id, keep)),
            None => questions.push(SyncQuestion {
                kind: SyncQuestionKind::MergedMatchInvalid,
                record,
                match_id: Some(*id),
                deleted_on: None,
            }),
        }
    }
    if !questions.is_empty() {
        return questions_result(questions);
    }

    for (id, keep) in replacements {
        let (kept, other) = match keep {
            SyncSide::Local => (&local_side, &remote_side),
            SyncSide::Remote => (&remote_side, &local_side),
        };
        replace_whole_match(id, kept, other, &mut matches, &mut facts, now);
    }

    SyncReconcileResult::Merged {
        snapshot: SyncSnapshot {
            matches: matches.into_values().collect(),
            teams: teams.into_values().collect(),
            players: players.into_values().collect(),
            facts: facts.into_values().collect(),
        },
    }
}

// ── 記録ごとの判定 ──

type Answers = HashMap<(SyncQuestionKind, SyncRecordRef), SyncSide>;

struct Context<'a> {
    answers: &'a Answers,
    now: DateTime<Utc>,
}

enum Decision<T> {
    Take(T),
    Ask(SyncQuestion),
}

trait Record: Clone {
    fn stamp(&self) -> SyncStamp;
    fn restamped(&self, stamp: SyncStamp) -> Self;
    fn same_content(&self, other: &Self) -> bool;
}

impl Record for SyncMatch {
    fn stamp(&self) -> SyncStamp {
        self.stamp
    }
    fn restamped(&self, stamp: SyncStamp) -> Self {
        SyncMatch {
            stamp,
            ..self.clone()
        }
    }
    fn same_content(&self, other: &Self) -> bool {
        match_content_equal(self, other)
    }
}

impl Record for SyncTeam {
    fn stamp(&self) -> SyncStamp {
        self.stamp
    }
    fn restamped(&self, stamp: SyncStamp) -> Self {
        SyncTeam {
            stamp,
            ..self.clone()
        }
    }
    fn same_content(&self, other: &Self) -> bool {
        team_content_equal(self, other)
    }
}

impl Record for SyncPlayer {
    fn stamp(&self) -> SyncStamp {
        self.stamp
    }
    fn restamped(&self, stamp: SyncStamp) -> Self {
        SyncPlayer {
            stamp,
            ..self.clone()
        }
    }
    fn same_content(&self, other: &Self) -> bool {
        player_content_equal(self, other)
    }
}

impl Record for SyncFact {
    fn stamp(&self) -> SyncStamp {
        self.stamp
    }
    fn restamped(&self, stamp: SyncStamp) -> Self {
        SyncFact {
            stamp,
            ..self.clone()
        }
    }
    fn same_content(&self, other: &Self) -> bool {
        fact_content_equal(self, other)
    }
}

struct Pair<'a, T> {
    local: Option<&'a T>,
    remote: Option<&'a T>,
}

/// 両方の記録を ID でまとめる。同じ側に同じ ID が 2 件あれば後の方を使う。
fn pairs<'a, T, K: Ord>(
    local: &'a [T],
    remote: &'a [T],
    key: impl Fn(&T) -> K,
) -> BTreeMap<K, Pair<'a, T>> {
    let mut map: BTreeMap<K, Pair<'a, T>> = BTreeMap::new();
    for item in local {
        map.entry(key(item))
            .or_insert(Pair {
                local: None,
                remote: None,
            })
            .local = Some(item);
    }
    for item in remote {
        map.entry(key(item))
            .or_insert(Pair {
                local: None,
                remote: None,
            })
            .remote = Some(item);
    }
    map
}

/// 1 件の記録を決める。両側とも無ければ `None`。
///
/// `last_changed` は「消されていない側の最後の変更」を記録自身の `updated_at` より広く取るときに
/// 渡す（試合は fact の変更も含める — 試合の `updated_at` は fact の変更で進まないため）。
fn decide<T: Record>(
    pair: &Pair<'_, T>,
    last_changed: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    record: SyncRecordRef,
    match_id: Option<MatchId>,
    ctx: &Context<'_>,
) -> Option<Decision<T>> {
    let (local, remote) = match (pair.local, pair.remote) {
        (None, None) => return None,
        (Some(only), None) | (None, Some(only)) => return Some(Decision::Take(only.clone())),
        (Some(local), Some(remote)) => (local, remote),
    };
    let (local_stamp, remote_stamp) = (local.stamp(), remote.stamp());
    let newer = |l: &T, r: &T| {
        if r.stamp().updated_at > l.stamp().updated_at {
            r.clone()
        } else {
            l.clone()
        }
    };
    let decision = match (local_stamp.deleted_at, remote_stamp.deleted_at) {
        (Some(_), Some(_)) => Decision::Take(newer(local, remote)),
        (None, None) => {
            if local.same_content(remote) {
                Decision::Take(newer(local, remote))
            } else if local_stamp.updated_at > remote_stamp.updated_at {
                Decision::Take(local.clone())
            } else if remote_stamp.updated_at > local_stamp.updated_at {
                Decision::Take(remote.clone())
            } else {
                ask(
                    SyncQuestionKind::SameTime,
                    record,
                    match_id,
                    None,
                    ctx,
                    |keep| match keep {
                        SyncSide::Local => local.restamped(SyncStamp::alive_now(ctx.now)),
                        SyncSide::Remote => remote.restamped(SyncStamp::alive_now(ctx.now)),
                    },
                )
            }
        }
        (Some(deleted_at), None) => deletion_against_change(
            (local, SyncSide::Local, deleted_at),
            remote,
            last_changed.1.unwrap_or(remote_stamp.updated_at),
            record,
            match_id,
            ctx,
        ),
        (None, Some(deleted_at)) => deletion_against_change(
            (remote, SyncSide::Remote, deleted_at),
            local,
            last_changed.0.unwrap_or(local_stamp.updated_at),
            record,
            match_id,
            ctx,
        ),
    };
    Some(decision)
}

/// 片方で消し、もう片方では消していないとき。消した時刻が相手の最後の変更より後なら消す。
fn deletion_against_change<T: Record>(
    (deleted, deleted_on, deleted_at): (&T, SyncSide, DateTime<Utc>),
    alive: &T,
    alive_last_changed: DateTime<Utc>,
    record: SyncRecordRef,
    match_id: Option<MatchId>,
    ctx: &Context<'_>,
) -> Decision<T> {
    if deleted_at >= alive_last_changed {
        return Decision::Take(deleted.clone());
    }
    ask(
        SyncQuestionKind::DeletedThenChanged,
        record,
        match_id,
        Some(deleted_on),
        ctx,
        |keep| {
            if keep == deleted_on {
                deleted.restamped(SyncStamp::deleted_now(ctx.now))
            } else {
                alive.restamped(SyncStamp::alive_now(ctx.now))
            }
        },
    )
}

fn ask<T>(
    kind: SyncQuestionKind,
    record: SyncRecordRef,
    match_id: Option<MatchId>,
    deleted_on: Option<SyncSide>,
    ctx: &Context<'_>,
    choose: impl FnOnce(SyncSide) -> T,
) -> Decision<T> {
    match ctx.answers.get(&(kind, record)) {
        Some(&keep) => Decision::Take(choose(keep)),
        None => Decision::Ask(SyncQuestion {
            kind,
            record,
            match_id,
            deleted_on,
        }),
    }
}

fn questions_result(mut questions: Vec<SyncQuestion>) -> SyncReconcileResult {
    questions.sort_by_key(|q| (q.match_id, q.record, q.kind));
    SyncReconcileResult::Questions { questions }
}

/// 試合ごとの「最後に変わった時刻」。試合自身と、その試合の fact（消した fact を含む）の
/// `updated_at` の最大。試合の削除と、相手での変更を比べるのに使う。
fn last_changed_by_match(snapshot: &SyncSnapshot) -> BTreeMap<MatchId, DateTime<Utc>> {
    let mut last: BTreeMap<MatchId, DateTime<Utc>> = BTreeMap::new();
    for m in &snapshot.matches {
        bump(&mut last, m.match_.id, m.stamp.updated_at);
    }
    for f in &snapshot.facts {
        bump(&mut last, f.match_id, f.stamp.updated_at);
    }
    last
}

fn bump(last: &mut BTreeMap<MatchId, DateTime<Utc>>, id: MatchId, at: DateTime<Utc>) {
    last.entry(id)
        .and_modify(|current| {
            if at > *current {
                *current = at;
            }
        })
        .or_insert(at);
}

// ── 参照の整合 ──

/// 消されていない試合・fact・選手が参照するチームと選手は、消されていても戻す（`now` で書き直す）。
///
/// 片方でチームを消し（使われていなかった）、もう片方でそのチームの試合を足していた、のような
/// 組み合わせで起きる。参照先の無い記録を残さないことを、消したことより優先する。
fn revive_referenced(
    matches: &BTreeMap<MatchId, SyncMatch>,
    facts: &BTreeMap<FactId, SyncFact>,
    teams: &mut BTreeMap<TeamId, SyncTeam>,
    players: &mut BTreeMap<PlayerId, SyncPlayer>,
    now: DateTime<Utc>,
) {
    let live_matches: BTreeSet<MatchId> = matches
        .values()
        .filter(|m| !m.stamp.is_deleted())
        .map(|m| m.match_.id)
        .collect();

    let mut needed_players: BTreeSet<PlayerId> = BTreeSet::new();
    for f in facts.values() {
        if f.stamp.is_deleted() || !live_matches.contains(&f.match_id) {
            continue;
        }
        if let MatchFactPayload::Play(play) = &f.fact.payload {
            needed_players.extend(play.player_id);
            needed_players.extend(play.related_player_id);
        }
    }
    for id in &needed_players {
        if let Some(player) = players.get_mut(id)
            && player.stamp.is_deleted()
        {
            *player = player.restamped(SyncStamp::alive_now(now));
        }
    }

    let mut needed_teams: BTreeSet<TeamId> = BTreeSet::new();
    for m in matches.values().filter(|m| !m.stamp.is_deleted()) {
        needed_teams.insert(m.match_.home_team_id);
        needed_teams.insert(m.match_.away_team_id);
    }
    for f in facts.values() {
        if f.stamp.is_deleted() || !live_matches.contains(&f.match_id) {
            continue;
        }
        match &f.fact.payload {
            MatchFactPayload::Play(play) => needed_teams.extend(play.team_id),
            MatchFactPayload::Possession(possession) => {
                needed_teams.insert(possession.team_id);
            }
            MatchFactPayload::Control(_) => {}
        }
    }
    for p in players.values().filter(|p| !p.stamp.is_deleted()) {
        needed_teams.insert(p.player.team_id);
    }
    for id in &needed_teams {
        if let Some(team) = teams.get_mut(id)
            && team.stamp.is_deleted()
        {
            *team = team.restamped(SyncStamp::alive_now(now));
        }
    }
}

// ── 混ざった試合の検証 ──

/// 片方の端末の、試合ごとの版（試合と消されていない fact）。
struct SideView<'a> {
    matches: BTreeMap<MatchId, &'a SyncMatch>,
    all_facts: BTreeMap<MatchId, Vec<&'a SyncFact>>,
}

impl<'a> SideView<'a> {
    fn new(
        match_pairs: &BTreeMap<MatchId, Pair<'a, SyncMatch>>,
        side: impl Fn(&Pair<'a, SyncMatch>) -> Option<&'a SyncMatch>,
        facts: &'a [SyncFact],
    ) -> SideView<'a> {
        let matches = match_pairs
            .iter()
            .filter_map(|(id, pair)| side(pair).map(|m| (*id, m)))
            .collect();
        let mut all_facts: BTreeMap<MatchId, Vec<&'a SyncFact>> = BTreeMap::new();
        for f in facts {
            all_facts.entry(f.match_id).or_default().push(f);
        }
        SideView { matches, all_facts }
    }

    /// この端末の版が、そろえた試合（試合と消されていない fact）とまったく同じか。
    fn equals(&self, id: MatchId, merged_match: &SyncMatch, merged_facts: &[&SyncFact]) -> bool {
        let Some(own) = self.matches.get(&id) else {
            return false;
        };
        if own.stamp.is_deleted() || !match_content_equal(own, merged_match) {
            return false;
        }
        let own_facts: BTreeMap<FactId, &SyncFact> = self
            .all_facts
            .get(&id)
            .into_iter()
            .flatten()
            .filter(|f| !f.stamp.is_deleted())
            .map(|f| (f.fact.id, *f))
            .collect();
        own_facts.len() == merged_facts.len()
            && merged_facts.iter().all(|merged| {
                own_facts
                    .get(&merged.fact.id)
                    .is_some_and(|own| fact_content_equal(own, merged))
            })
    }
}

fn live_facts_of(facts: &BTreeMap<FactId, SyncFact>, match_id: MatchId) -> Vec<&SyncFact> {
    facts
        .values()
        .filter(|f| f.match_id == match_id && !f.stamp.is_deleted())
        .collect()
}

/// そろえた試合が規則（試合・fact 1 件ずつ・fact の列全体）に合わないか。
///
/// 選手の所属は「その試合の両チームに居る、消されていない選手」だけを見る。存在しない選手への
/// 参照（dangling）は見ない — 選手は試合と別の記録で、片方の端末で消した選手を参照する fact は
/// [`revive_referenced`] が選手を戻して解消している。
fn merged_match_has_issues(
    merged_match: &SyncMatch,
    merged_facts: &[&SyncFact],
    players: &BTreeMap<PlayerId, SyncPlayer>,
) -> bool {
    let match_ = &merged_match.match_;
    let lookup: BTreeMap<PlayerId, TeamId> = players
        .values()
        .filter(|p| {
            !p.stamp.is_deleted()
                && (p.player.team_id == match_.home_team_id
                    || p.player.team_id == match_.away_team_id)
        })
        .map(|p| (p.player.id, p.player.team_id))
        .collect();
    let roster = RosterContext {
        home_team_id: match_.home_team_id,
        away_team_id: match_.away_team_id,
        player_team_lookup: lookup,
        known_player_ids: None,
    };
    let facts: Vec<MatchFact> = merged_facts.iter().map(|f| f.fact.clone()).collect();
    let ordered = persistence_ordered(&facts);

    if !validate_match(match_).is_empty() {
        return true;
    }
    if ordered
        .iter()
        .any(|fact| !validate_match_fact(fact, &match_.configuration, &roster).is_empty())
    {
        return true;
    }
    !validate_fact_log(&ordered, match_).is_empty()
}

/// `MergedMatchInvalid` に答えた側の試合を丸ごと採る。
///
/// 採った側の試合と fact はすべて `now` で書き直し（次の同期で、まだ古い版を持つ端末に負けない
/// ように）、採らなかった側にしか無い fact は消す。
fn replace_whole_match(
    id: MatchId,
    kept: &SideView<'_>,
    other: &SideView<'_>,
    matches: &mut BTreeMap<MatchId, SyncMatch>,
    facts: &mut BTreeMap<FactId, SyncFact>,
    now: DateTime<Utc>,
) {
    let restamp = |stamp: SyncStamp| {
        if stamp.is_deleted() {
            SyncStamp::deleted_now(now)
        } else {
            SyncStamp::alive_now(now)
        }
    };
    if let Some(kept_match) = kept.matches.get(&id) {
        matches.insert(id, kept_match.restamped(restamp(kept_match.stamp)));
    }
    facts.retain(|_, f| f.match_id != id);
    let kept_facts = kept.all_facts.get(&id).into_iter().flatten();
    let mut kept_ids: BTreeSet<FactId> = BTreeSet::new();
    for f in kept_facts {
        kept_ids.insert(f.fact.id);
        facts.insert(f.fact.id, f.restamped(restamp(f.stamp)));
    }
    for f in other.all_facts.get(&id).into_iter().flatten() {
        if !kept_ids.contains(&f.fact.id) {
            facts.insert(f.fact.id, f.restamped(SyncStamp::deleted_now(now)));
        }
    }
}
