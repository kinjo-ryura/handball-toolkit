//! 突き合わせ — 端末をまたぐ、ID の違う同じ試合の写しを 1 つにする（ADR 0007 決定 7）。
//!
//! 試合ファイル（SAMPLE_DTO_V2）で受け取った写しは、試合・fact・チーム・選手とも元の端末と別の ID を
//! 持つ。そのまま記録ごとに比べると、全部が「片方にしか無い」になり、同じ試合が 2 つ並ぶ。
//!
//! - **同じ試合の見分け方**: fact の「記録した時刻と種類」（印）が、印の少ない側の半分以上で
//!   一致するか。取り込みは `recorded_at` を残すが、試合ファイルは秒までしか書かない（切り捨て）ので、
//!   写しの時刻は秒ちょうどになる。印が一致するのは、種類と秒が同じで、ミリ秒まで同じか、片方が
//!   秒ちょうどのとき。端末で記録した時刻はミリ秒まで持つので、同じ試合を 2 台で別々に記録しても
//!   （前半開始を同じ秒に押しても）ミリ秒が食い違って一致しない。半分以上を求めるのは、別の人の記録の
//!   試合ファイル（秒ちょうど）が手元の記録と偶然同じ秒に当たることがあるため。写しなら、どちらかで
//!   足したり消したりしていても、少ない側の印のほとんどが一致する。fact の無い試合は、日付（秒）と
//!   両チームの名前で見る
//! - **1 つにするのは、まだ同期していない端末をまたぐ写しの組だけ**:
//!   - どちらかの ID が両方の端末にある（同期済みの試合に、後から試合ファイルで写しが届いた）→
//!     まとめない。写しは別の試合として足し、[`super::SyncDuplicateGroup`] で知らせる
//!   - 同じ端末の中の写しどうし → まとめない（同上で知らせる）
//!   - 端末をまたぐ写しが組をなす → 中身が同じなら小さい方の ID に黙ってまとめ、違えば
//!     [`super::SyncQuestionKind::CopiesDiffer`] で聞いて、答えた側を残す
//! - **チームと選手**: 組にした試合のホーム同士・アウェイ同士で、名前が同じチームを 1 つにする。
//!   つながったチームは向きを見ずに 1 つの組にし、組ごとに行き先を 1 つ選ぶ（2 台が互いに試合ファイルを
//!   送り合っていると、組ごとに残す側が逆になるため — `Edits::team_merges`）。
//!   選手は背番号と名前が同じ人をまとめ、残りはまとめた先のチームへ移す。名前の違うチーム
//!   （取り込みで既存の別名のチームに紐付けたもの）はまとめない
//!
//! 消す側の写し（試合・その fact・まとめたチームと選手）は `now` で消し、組の残す側を問いで選んだときは
//! 残す試合とその fact を `now` で書き直す。どちらも、まだ古い写しを持つ端末と後で比べても負けないように。

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::clock::FactAnchor;
use crate::configuration::{MatchConfiguration, VideoProvider};
use crate::entities::Match;
use crate::facts::{ControlFact, MatchFact, MatchFactPayload};
use crate::ids::{MatchId, PlayerId, TeamId};

use super::changes::IdMerges;
use super::compare::same_local_video;
use super::{
    Answers, SyncFact, SyncMatch, SyncPlayer, SyncQuestion, SyncQuestionKind, SyncRecordRef,
    SyncSide, SyncSnapshot, SyncStamp, SyncTeam,
};

/// 突き合わせた後の両端末の記録と、聞くこと・知らせること。
pub(crate) struct Pairing {
    pub(crate) local: SyncSnapshot,
    pub(crate) remote: SyncSnapshot,
    pub(crate) questions: Vec<SyncQuestion>,
    /// まとめずに残した、同じ試合に見える組（試合 ID の昇順）。
    pub(crate) duplicates: Vec<Vec<MatchId>>,
    /// 端末ごとの、まとめた写し・チーム・選手の ID の読み替え（一覧 — `changes`）。
    pub(crate) local_merges: IdMerges,
    pub(crate) remote_merges: IdMerges,
}

pub(crate) fn pair_copies(
    local: &SyncSnapshot,
    remote: &SyncSnapshot,
    answers: &Answers,
    now: DateTime<Utc>,
) -> Pairing {
    let local_view = SideView::new(SyncSide::Local, local);
    let remote_view = SideView::new(SyncSide::Remote, remote);
    let nodes: Vec<Node> = local_view
        .nodes()
        .into_iter()
        .chain(remote_view.nodes())
        .collect();
    let views = Views {
        local: &local_view,
        remote: &remote_view,
    };

    let mut questions: Vec<SyncQuestion> = Vec::new();
    let mut duplicates: Vec<Vec<MatchId>> = Vec::new();
    let mut edits = Edits::default();

    for cluster in clusters(&nodes) {
        let ids: BTreeSet<MatchId> = cluster.iter().map(|&i| nodes[i].id).collect();
        if ids.len() < 2 {
            continue;
        }
        let local_members: Vec<&Node> = cluster
            .iter()
            .map(|&i| &nodes[i])
            .filter(|n| n.side == SyncSide::Local)
            .collect();
        let remote_members: Vec<&Node> = cluster
            .iter()
            .map(|&i| &nodes[i])
            .filter(|n| n.side == SyncSide::Remote)
            .collect();
        let local_ids: BTreeSet<MatchId> = local_members.iter().map(|n| n.id).collect();
        let shared = remote_members.iter().any(|n| local_ids.contains(&n.id));
        if shared || local_members.is_empty() || remote_members.is_empty() {
            duplicates.push(ids.into_iter().collect());
            continue;
        }

        let Some((l, r, identical)) = best_pair(&local_members, &remote_members, &views) else {
            duplicates.push(ids.into_iter().collect());
            continue;
        };
        let record = SyncRecordRef::Match { id: l.id.min(r.id) };
        let (survivor, loser, restamp_survivor) = if identical {
            if l.id < r.id {
                (l, r, false)
            } else {
                (r, l, false)
            }
        } else {
            match answers.get(&(SyncQuestionKind::CopiesDiffer, record)) {
                Some(SyncSide::Local) => (l, r, true),
                Some(SyncSide::Remote) => (r, l, true),
                None => {
                    questions.push(SyncQuestion {
                        kind: SyncQuestionKind::CopiesDiffer,
                        record,
                        match_id: Some(l.id.min(r.id)),
                        deleted_on: None,
                    });
                    continue;
                }
            }
        };

        edits.replace_match(loser.side, loser.id, survivor.id, identical);
        if restamp_survivor {
            edits.restamp_match(survivor.side, survivor.id);
        }
        let survivor_view = views.get(survivor.side);
        let loser_view = views.get(loser.side);
        if let (Some(kept), Some(dropped)) = (
            survivor_view.matches.get(&survivor.id),
            loser_view.matches.get(&loser.id),
        ) {
            for (dropped_team, kept_team) in [
                (dropped.match_.home_team_id, kept.match_.home_team_id),
                (dropped.match_.away_team_id, kept.match_.away_team_id),
            ] {
                if dropped_team != kept_team
                    && loser_view.team_name(dropped_team).is_some()
                    && loser_view.team_name(dropped_team) == survivor_view.team_name(kept_team)
                {
                    edits.vote_team(loser.side, dropped_team, survivor.side, kept_team);
                }
            }
        }

        let leftovers: Vec<MatchId> = ids.iter().copied().filter(|id| *id != loser.id).collect();
        if leftovers.len() > 1 {
            duplicates.push(leftovers);
        }
    }

    if !questions.is_empty() {
        return Pairing {
            local: local.clone(),
            remote: remote.clone(),
            questions,
            duplicates: Vec::new(),
            local_merges: IdMerges::default(),
            remote_merges: IdMerges::default(),
        };
    }

    let team_merges = edits.team_merges();
    let mut local_out = local.clone();
    let mut remote_out = remote.clone();
    let mut local_merges = IdMerges::default();
    let mut remote_merges = IdMerges::default();
    for (side, out, id_merges) in [
        (SyncSide::Local, &mut local_out, &mut local_merges),
        (SyncSide::Remote, &mut remote_out, &mut remote_merges),
    ] {
        let merges: BTreeMap<TeamId, (SyncSide, TeamId)> = team_merges
            .iter()
            .filter(|((s, _), _)| *s == side)
            .map(|((_, from), to)| (*from, *to))
            .collect();
        let players_to_merge = player_merges(views.get(side), &merges, &views);
        apply(out, side, &edits, &merges, &players_to_merge, now);
        *id_merges = IdMerges {
            matches: edits
                .replaced_matches
                .iter()
                .filter(|((s, _), _)| *s == side)
                .map(|((_, loser), survivor)| (*loser, *survivor))
                .collect(),
            teams: merges.iter().map(|(from, (_, to))| (*from, *to)).collect(),
            players: players_to_merge,
        };
    }

    Pairing {
        local: local_out,
        remote: remote_out,
        questions,
        duplicates,
        local_merges,
        remote_merges,
    }
}

// ── 写しの見分け ──

/// 突き合わせの単位: 片方の端末の、消されていない試合 1 つ。
struct Node {
    side: SyncSide,
    id: MatchId,
    /// fact の印: 「記録した秒と種類」→ その秒の中のミリ秒（0 は秒ちょうど）。
    prints: BTreeMap<(i64, String), BTreeSet<u32>>,
    /// 印の数（ミリ秒まで数える）。
    print_count: usize,
    /// fact の無い試合だけ、日付（秒）と両チームの名前。
    empty_key: Option<(i64, String, String)>,
}

/// 2 つの試合が同じ試合に見えるか。見えるなら、一致する印の数（fact の無い試合どうしは 0）。
///
/// 印が一致するのは種類と秒が同じで、ミリ秒まで同じか、片方が秒ちょうど（試合ファイルで受け取った
/// 写し）のとき。一致する印が、印の少ない側の半分以上あれば同じ試合とみなす（モジュールの doc）。
fn linked(a: &Node, b: &Node) -> Option<usize> {
    if let (Some(x), Some(y)) = (&a.empty_key, &b.empty_key) {
        return (x == y).then_some(0);
    }
    let shared: usize = a
        .prints
        .iter()
        .filter_map(|(key, a_millis)| b.prints.get(key).map(|b_millis| (a_millis, b_millis)))
        .map(|(a_millis, b_millis)| matched_prints(a_millis, b_millis))
        .sum();
    let fewer = a.print_count.min(b.print_count);
    (shared > 0 && shared * 2 >= fewer).then_some(shared)
}

/// 同じ秒・同じ種類の印どうしで、1 対 1 に一致させられる最大の数。
///
/// ミリ秒が同じ印どうしは一致する。秒ちょうど（0）の印は、相手のどの印とも一致できる。
fn matched_prints(a: &BTreeSet<u32>, b: &BTreeSet<u32>) -> usize {
    let (a_whole, b_whole) = (a.contains(&0), b.contains(&0));
    let exact = a.iter().filter(|m| **m != 0 && b.contains(m)).count();
    // ミリ秒の同じ相手が無い、秒ちょうどでない印の数。
    let a_rest = a.len() - usize::from(a_whole) - exact;
    let b_rest = b.len() - usize::from(b_whole) - exact;
    let whole = match (a_whole, b_whole) {
        // 秒ちょうどどうしで 1 つ、または両方がそれぞれ相手の残りと 1 つずつ。
        (true, true) => 1 + usize::from(a_rest > 0 && b_rest > 0),
        (true, false) => usize::from(b_rest > 0),
        (false, true) => usize::from(a_rest > 0),
        (false, false) => 0,
    };
    exact + whole
}

fn kind_key(fact: &MatchFact) -> String {
    match &fact.payload {
        MatchFactPayload::Play(play) => format!("play:{:?}", play.kind),
        MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => {
            format!("phaseStart:{:?}", payload.kind)
        }
        MatchFactPayload::Control(ControlFact::Stoppage(payload)) => {
            format!("stoppage:{:?}", payload.kind)
        }
        MatchFactPayload::Possession(_) => "possession".to_owned(),
    }
}

/// 同じ試合に見える試合をまとめる（同じ ID・[`linked`]・fact の無い試合の日付とチーム名）。
fn clusters(nodes: &[Node]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    let mut first_by_id: BTreeMap<MatchId, usize> = BTreeMap::new();
    let mut by_print: BTreeMap<&(i64, String), Vec<usize>> = BTreeMap::new();
    let mut first_by_empty: BTreeMap<&(i64, String, String), usize> = BTreeMap::new();
    for (i, node) in nodes.iter().enumerate() {
        if let Some(&j) = first_by_id.get(&node.id) {
            union(&mut parent, i, j);
        } else {
            first_by_id.insert(node.id, i);
        }
        for key in node.prints.keys() {
            by_print.entry(key).or_default().push(i);
        }
        if let Some(key) = &node.empty_key {
            if let Some(&j) = first_by_empty.get(key) {
                union(&mut parent, i, j);
            } else {
                first_by_empty.insert(key, i);
            }
        }
    }
    // 秒と種類を共有する試合の組だけを確かめる。
    let mut checked: BTreeSet<(usize, usize)> = BTreeSet::new();
    for members in by_print.values() {
        for (k, &i) in members.iter().enumerate() {
            for &j in &members[k + 1..] {
                if checked.insert((i, j)) && linked(&nodes[i], &nodes[j]).is_some() {
                    union(&mut parent, i, j);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..nodes.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    groups.into_values().collect()
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

/// 組の良さ: 一致する印の数 → 中身が同じか → ID が小さいか（`Reverse` で小さい方を大きく見せる）。
type PairScore = (usize, bool, std::cmp::Reverse<(MatchId, MatchId)>);

/// 端末をまたいで組にする 2 つ。同じ試合に見える組（[`linked`]）のうち、印を最も多く共有する組、
/// 同じなら中身が同じ組、さらに同じなら ID の小さい組。戻り値の最後は「中身が同じか」。
///
/// 同じ端末の写しを介して 1 つにまとまった試合どうしは、直接には同じ試合に見えないことがあるので組にしない。
fn best_pair<'n>(
    local_members: &[&'n Node],
    remote_members: &[&'n Node],
    views: &Views<'_>,
) -> Option<(&'n Node, &'n Node, bool)> {
    let mut best: Option<(PairScore, &Node, &Node)> = None;
    for &l in local_members {
        for &r in remote_members {
            let Some(shared) = linked(l, r) else {
                continue;
            };
            let identical = copies_identical(views.get(l.side), l.id, views.get(r.side), r.id);
            let score = (shared, identical, std::cmp::Reverse((l.id, r.id)));
            if best.as_ref().is_none_or(|(current, _, _)| score > *current) {
                best = Some((score, l, r));
            }
        }
    }
    best.map(|((_, identical, _), l, r)| (l, r, identical))
}

/// 2 つの写しの中身が同じか。ID は写しごとに違うので、チームは名前、選手は名前と背番号、fact は
/// 記録した秒と中身（時刻はミリ秒まで）で比べる。端末ごとの値（左右配置・端末内動画の参照）は見ない。
fn copies_identical(a: &SideView<'_>, a_id: MatchId, b: &SideView<'_>, b_id: MatchId) -> bool {
    let (Some(am), Some(bm)) = (a.matches.get(&a_id), b.matches.get(&b_id)) else {
        return false;
    };
    let (x, y) = (&am.match_, &bm.match_);
    if x.title != y.title
        || x.date.timestamp() != y.date.timestamp()
        || a.team_name(x.home_team_id) != b.team_name(y.home_team_id)
        || a.team_name(x.away_team_id) != b.team_name(y.away_team_id)
        || !same_configuration(am, bm)
    {
        return false;
    }
    let signatures = |view: &SideView<'_>, m: &Match| -> Vec<String> {
        let mut list: Vec<String> = view
            .live_facts
            .get(&m.id)
            .into_iter()
            .flatten()
            .map(|f| fact_signature(&f.fact, view, m))
            .collect();
        list.sort();
        list
    };
    signatures(a, x) == signatures(b, y)
}

fn same_configuration(a: &SyncMatch, b: &SyncMatch) -> bool {
    match (&a.match_.configuration, &b.match_.configuration) {
        (MatchConfiguration::Video(sa), MatchConfiguration::Video(sb))
        | (MatchConfiguration::VideoHighlight(sa), MatchConfiguration::VideoHighlight(sb))
            if sa.provider == VideoProvider::Local && sb.provider == VideoProvider::Local =>
        {
            same_local_video(a.local_video.as_ref(), b.local_video.as_ref())
        }
        (x, y) => x == y,
    }
}

fn fact_signature(fact: &MatchFact, view: &SideView<'_>, m: &Match) -> String {
    let secs = fact.recorded_at.timestamp();
    match &fact.payload {
        MatchFactPayload::Play(play) => format!(
            "{secs}|play|{:?}|{}|{}|{}|{}|{:?}|{:?}",
            play.kind,
            anchor_key(&play.anchor),
            team_side(play.team_id, m),
            view.player_key(play.player_id),
            view.player_key(play.related_player_id),
            play.title,
            play.note,
        ),
        MatchFactPayload::Control(ControlFact::PhaseStart(payload)) => format!(
            "{secs}|phaseStart|{:?}|{}|{}",
            payload.kind,
            anchor_key(&payload.start_anchor),
            anchor_key(&payload.end_anchor),
        ),
        MatchFactPayload::Control(ControlFact::Stoppage(payload)) => format!(
            "{secs}|stoppage|{:?}|{}|{}|{:?}",
            payload.kind,
            anchor_key(&payload.start_anchor),
            payload
                .end_anchor
                .as_ref()
                .map(anchor_key)
                .unwrap_or_default(),
            payload.note,
        ),
        MatchFactPayload::Possession(possession) => format!(
            "{secs}|possession|{}|{}|{}",
            team_side(Some(possession.team_id), m),
            anchor_key(&possession.anchor),
            possession
                .end_anchor
                .as_ref()
                .map(anchor_key)
                .unwrap_or_default(),
        ),
    }
}

/// anchor の秒をミリ秒まで（写しの往復で末尾の桁がずれても同じと見る）。
fn anchor_key(anchor: &FactAnchor) -> String {
    let millis = |seconds: Option<f64>| seconds.map(|s| format!("{s:.3}")).unwrap_or_default();
    format!(
        "{}/{}",
        millis(anchor.match_elapsed_seconds()),
        millis(anchor.video_elapsed_seconds())
    )
}

fn team_side(team: Option<TeamId>, m: &Match) -> &'static str {
    match team {
        None => "-",
        Some(t) if t == m.home_team_id => "home",
        Some(t) if t == m.away_team_id => "away",
        Some(_) => "other",
    }
}

// ── 片方の端末 ──

/// 両端末のビュー。
struct Views<'v> {
    local: &'v SideView<'v>,
    remote: &'v SideView<'v>,
}

impl<'v> Views<'v> {
    fn get(&self, side: SyncSide) -> &'v SideView<'v> {
        match side {
            SyncSide::Local => self.local,
            SyncSide::Remote => self.remote,
        }
    }
}

struct SideView<'a> {
    side: SyncSide,
    matches: BTreeMap<MatchId, &'a SyncMatch>,
    teams: BTreeMap<TeamId, &'a SyncTeam>,
    players: BTreeMap<PlayerId, &'a SyncPlayer>,
    live_facts: BTreeMap<MatchId, Vec<&'a SyncFact>>,
}

impl<'a> SideView<'a> {
    fn new(side: SyncSide, snapshot: &'a SyncSnapshot) -> SideView<'a> {
        let mut live_facts: BTreeMap<MatchId, Vec<&'a SyncFact>> = BTreeMap::new();
        for f in snapshot.facts.iter().filter(|f| !f.stamp.is_deleted()) {
            live_facts.entry(f.match_id).or_default().push(f);
        }
        SideView {
            side,
            matches: snapshot
                .matches
                .iter()
                .filter(|m| !m.stamp.is_deleted())
                .map(|m| (m.match_.id, m))
                .collect(),
            teams: snapshot.teams.iter().map(|t| (t.team.id, t)).collect(),
            players: snapshot.players.iter().map(|p| (p.player.id, p)).collect(),
            live_facts,
        }
    }

    fn nodes(&self) -> Vec<Node> {
        self.matches
            .values()
            .map(|m| {
                let mut prints: BTreeMap<(i64, String), BTreeSet<u32>> = BTreeMap::new();
                for f in self.live_facts.get(&m.match_.id).into_iter().flatten() {
                    let at = f.fact.recorded_at;
                    prints
                        .entry((at.timestamp(), kind_key(&f.fact)))
                        .or_default()
                        .insert(at.timestamp_subsec_millis());
                }
                let print_count: usize = prints.values().map(BTreeSet::len).sum();
                let empty_key = prints.is_empty().then(|| {
                    (
                        m.match_.date.timestamp(),
                        self.team_name(m.match_.home_team_id)
                            .unwrap_or_default()
                            .to_owned(),
                        self.team_name(m.match_.away_team_id)
                            .unwrap_or_default()
                            .to_owned(),
                    )
                });
                Node {
                    side: self.side,
                    id: m.match_.id,
                    prints,
                    print_count,
                    empty_key,
                }
            })
            .collect()
    }

    fn team_name(&self, id: TeamId) -> Option<&str> {
        self.teams.get(&id).map(|t| t.team.name.trim())
    }

    fn player_key(&self, id: Option<PlayerId>) -> String {
        match id {
            None => "-".to_owned(),
            Some(id) => self.players.get(&id).map_or_else(
                || "?".to_owned(),
                |p| format!("{}#{:?}", p.player.name.trim(), p.player.jersey_number),
            ),
        }
    }
}

// ── 書き換え ──

#[derive(Default)]
struct Edits {
    /// 消す写し（端末・試合）。
    dropped_matches: BTreeSet<(SyncSide, MatchId)>,
    /// 消す写し（端末・試合）→ (残す写し, 中身が同じか)。
    replaced_matches: BTreeMap<(SyncSide, MatchId), (MatchId, bool)>,
    /// 問いで残すと決めた写し（`now` で書き直す）。
    restamped_matches: BTreeSet<(SyncSide, MatchId)>,
    /// (端末, まとめられるチーム) → { (まとめ先の端末, まとめ先のチーム) → 票 }
    team_votes: BTreeMap<(SyncSide, TeamId), BTreeMap<(SyncSide, TeamId), usize>>,
}

impl Edits {
    /// `side` の写し `id` を消し、`survivor`（相手の端末の写し）を残す。
    fn replace_match(&mut self, side: SyncSide, id: MatchId, survivor: MatchId, identical: bool) {
        self.dropped_matches.insert((side, id));
        self.replaced_matches
            .insert((side, id), (survivor, identical));
    }

    fn restamp_match(&mut self, side: SyncSide, id: MatchId) {
        self.restamped_matches.insert((side, id));
    }

    fn vote_team(&mut self, side: SyncSide, from: TeamId, to_side: SyncSide, to: TeamId) {
        *self
            .team_votes
            .entry((side, from))
            .or_default()
            .entry((to_side, to))
            .or_default() += 1;
    }

    /// まとめるチームの行き先。
    ///
    /// 票は「消す写しのチーム → 残す写しのチーム」の向きを持つが、向きのまま行き先にすると、2 台が
    /// 互いに試合ファイルを送り合っていたとき（組ごとに残す側が逆になる）`A → B` と `B → A` が両方でき、
    /// 両方のチームを消して付け替え合う。参照のある両方を `revive_referenced`（`reconcile.rs`）が戻すので、
    /// 同じ名前のチームが 2 つ残る（handball-project#510）。
    ///
    /// そこで向きを捨て、票でつながったチームを 1 つの組にして、組ごとに行き先を 1 つ選ぶ: 残す側として
    /// 票を最も多く受けたチーム、同じなら ID の小さいチーム。組のほかのチームはすべてそこへまとめる
    /// （両方の端末にある同じ ID のチームは同じチームなので、まとめない）。
    fn team_merges(&self) -> BTreeMap<(SyncSide, TeamId), (SyncSide, TeamId)> {
        let teams: Vec<(SyncSide, TeamId)> = self
            .team_votes
            .iter()
            .flat_map(|(from, votes)| std::iter::once(*from).chain(votes.keys().copied()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let index: BTreeMap<(SyncSide, TeamId), usize> = teams
            .iter()
            .enumerate()
            .map(|(i, team)| (*team, i))
            .collect();
        let mut parent: Vec<usize> = (0..teams.len()).collect();
        let mut received: Vec<usize> = vec![0; teams.len()];
        for (from, votes) in &self.team_votes {
            for (to, count) in votes {
                union(&mut parent, index[from], index[to]);
                received[index[to]] += count;
            }
        }
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..teams.len() {
            let root = find(&mut parent, i);
            groups.entry(root).or_default().push(i);
        }

        let mut merges: BTreeMap<(SyncSide, TeamId), (SyncSide, TeamId)> = BTreeMap::new();
        for members in groups.values() {
            let Some(&target) = members.iter().max_by(|&&a, &&b| {
                received[a]
                    .cmp(&received[b])
                    .then_with(|| teams[b].1.cmp(&teams[a].1))
                    .then_with(|| teams[b].0.cmp(&teams[a].0))
            }) else {
                continue;
            };
            let to = teams[target];
            for &i in members {
                if teams[i].1 != to.1 {
                    merges.insert(teams[i], to);
                }
            }
        }
        merges
    }
}

/// まとめるチームの選手を、まとめ先のチームの選手に対応させる（背番号と名前が同じ人）。
fn player_merges(
    from_view: &SideView<'_>,
    team_merges: &BTreeMap<TeamId, (SyncSide, TeamId)>,
    views: &Views<'_>,
) -> BTreeMap<PlayerId, PlayerId> {
    let mut merges: BTreeMap<PlayerId, PlayerId> = BTreeMap::new();
    for (from_team, (to_side, to_team)) in team_merges {
        let to_view = views.get(*to_side);
        let mut taken: BTreeSet<PlayerId> = BTreeSet::new();
        for p in from_view
            .players
            .values()
            .filter(|p| p.player.team_id == *from_team && !p.stamp.is_deleted())
        {
            let target = to_view.players.values().find(|q| {
                q.player.team_id == *to_team
                    && !q.stamp.is_deleted()
                    && !taken.contains(&q.player.id)
                    && q.player.jersey_number == p.player.jersey_number
                    && q.player.name.trim() == p.player.name.trim()
            });
            if let Some(q) = target {
                taken.insert(q.player.id);
                merges.insert(p.player.id, q.player.id);
            }
        }
    }
    merges
}

/// 片方の端末の記録に、消す写し・残す写しの書き直し・チームと選手のまとめを当てる。
fn apply(
    snapshot: &mut SyncSnapshot,
    side: SyncSide,
    edits: &Edits,
    team_merges: &BTreeMap<TeamId, (SyncSide, TeamId)>,
    player_merges: &BTreeMap<PlayerId, PlayerId>,
    now: DateTime<Utc>,
) {
    let team = |id: TeamId| team_merges.get(&id).map_or(id, |(_, to)| *to);
    let player = |id: PlayerId| player_merges.get(&id).copied().unwrap_or(id);

    for m in snapshot
        .matches
        .iter_mut()
        .filter(|m| !m.stamp.is_deleted())
    {
        let key = (side, m.match_.id);
        if edits.dropped_matches.contains(&key) {
            m.stamp = SyncStamp::deleted_now(now);
            continue;
        }
        let before = m.match_.clone();
        m.match_.home_team_id = team(m.match_.home_team_id);
        m.match_.away_team_id = team(m.match_.away_team_id);
        let selection = &mut m.match_.roster_selection;
        selection.benched_player_ids = selection
            .benched_player_ids
            .iter()
            .map(|id| player(*id))
            .collect();
        selection.out_of_roster_player_ids = selection
            .out_of_roster_player_ids
            .iter()
            .map(|id| player(*id))
            .collect();
        if m.match_ != before || edits.restamped_matches.contains(&key) {
            m.stamp = SyncStamp::alive_now(now);
        }
    }

    for f in snapshot.facts.iter_mut().filter(|f| !f.stamp.is_deleted()) {
        let key = (side, f.match_id);
        if edits.dropped_matches.contains(&key) {
            f.stamp = SyncStamp::deleted_now(now);
            continue;
        }
        let before = f.fact.clone();
        match &mut f.fact.payload {
            MatchFactPayload::Play(play) => {
                play.team_id = play.team_id.map(team);
                play.player_id = play.player_id.map(player);
                play.related_player_id = play.related_player_id.map(player);
            }
            MatchFactPayload::Possession(possession) => {
                possession.team_id = team(possession.team_id);
            }
            MatchFactPayload::Control(_) => {}
        }
        if f.fact != before || edits.restamped_matches.contains(&key) {
            f.stamp = SyncStamp::alive_now(now);
        }
    }

    for p in snapshot
        .players
        .iter_mut()
        .filter(|p| !p.stamp.is_deleted())
    {
        if player_merges.contains_key(&p.player.id) {
            p.stamp = SyncStamp::deleted_now(now);
        } else if let Some((_, to)) = team_merges.get(&p.player.team_id) {
            p.player.team_id = *to;
            p.stamp = SyncStamp::alive_now(now);
        }
    }

    for t in snapshot.teams.iter_mut().filter(|t| !t.stamp.is_deleted()) {
        if team_merges.contains_key(&t.team.id) {
            t.stamp = SyncStamp::deleted_now(now);
        }
    }
}
