//! 一覧 — そろえた中身を保存すると、それぞれの端末で何が変わるか（ADR 0007 決定 8。handball-project#520）。
//!
//! 始めた側は、そろえた中身（[`super::reconcile`] の `Merged`）を保存する前に、端末ごとの変化を見せる。
//! **どちらの端末にも変わるものが無ければ「すべて最新です」とし、保存しない**（シェルの判断）。
//!
//! 数えるのは、利用者に見える中身が変わるときだけ:
//!
//! | 数えない差 | 理由 |
//! |---|---|
//! | 削除の記録だけ（その端末で見えていなかった記録を、消した記録として受け取る） | 画面に何も出ない |
//! | 時刻だけ（中身が同じで `updated_at` だけが違う） | 同上 |
//! | 端末ごとの値（左右配置・写真・端末内動画の参照） | 保存しても端末の値が残る（[`super::materialize`]） |
//! | 消えている試合の記録（fact） | 試合ごと見えない |
//! | 同じ中身の写しを 1 つにまとめる | 見える試合・チーム・選手は変わらない（ID だけが変わる） |
//!
//! 見せる単位は試合とチーム。記録（fact）の変化は試合の「更新」に、選手の変化はチームの「更新」に
//! 入れて数を添える。
//!
//! 突き合わせ（`pairing`）でまとめた写し・チーム・選手は、消す側の ID を残す側の ID に読み替えてから
//! 比べる。読み替えないと、同じ試合やチームが「削除」と「追加」の 2 行に見える。

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::facts::{MatchFact, MatchFactPayload};
use crate::ids::{FactId, MatchId, PlayerId, TeamId};

use super::compare::configuration_equal;
use super::{SyncFact, SyncMatch, SyncPlayer, SyncSnapshot, SyncTeam};

/// 一覧の 1 行が、その端末で何をするか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum SyncChangeKind {
    /// この端末に無かった（消してあった）ものが入る
    Added,
    /// この端末にあるものの中身が変わる
    Updated,
    /// この端末にあるものが消える
    Removed,
}

/// 試合の「更新」で変わる項目（端末ごとの値は含めない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum SyncMatchField {
    Title,
    Date,
    /// ホーム・アウェイのチーム
    Teams,
    /// 記録の方法（タイマー / 動画 / ハイライト）と動画
    Video,
    /// 出場選手（ベンチ・ベンチ外）
    Roster,
}

/// 試合 1 つの変化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncMatchChange {
    /// 消えるときはこの端末の ID、ほかはそろえた中身の ID（写しが置き換わるときは残す側の ID）。
    pub match_id: MatchId,
    pub kind: SyncChangeKind,
    /// `Updated` のとき、変わる試合の項目。
    pub fields: Vec<SyncMatchField>,
    /// 記録（fact）。`Added` は入る試合の記録の数、`Updated` は足す・消す・変わる数、`Removed` は 0。
    pub facts_added: u32,
    pub facts_removed: u32,
    pub facts_changed: u32,
    /// `Updated` のとき、この端末の写しが、中身の違う相手の写しに置き換わる（`CopiesDiffer` の答え）。
    /// 項目と記録の数は数えない — 写しどうしは記録の ID が違い、どの記録が同じかを決められない。
    pub copy_replaced: bool,
}

/// チーム 1 つの変化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncTeamChange {
    /// 消えるときはこの端末の ID、ほかはそろえた中身の ID。
    pub team_id: TeamId,
    pub kind: SyncChangeKind,
    /// `Updated` のとき、名前が変わる。
    pub renamed: bool,
    /// 選手。`Added` は入るチームの選手の数、`Updated` は足す・消す・変わる（名前か背番号）数、
    /// `Removed` は 0。
    pub players_added: u32,
    pub players_removed: u32,
    pub players_changed: u32,
}

/// 1 台の端末の変化。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncDeviceChanges {
    /// 試合の日付の新しい順。
    pub matches: Vec<SyncMatchChange>,
    /// 名前順。
    pub teams: Vec<SyncTeamChange>,
}

impl SyncDeviceChanges {
    pub fn is_empty(&self) -> bool {
        self.matches.is_empty() && self.teams.is_empty()
    }
}

/// 両方の端末の変化。どちらも空なら「すべて最新です」で、保存しない。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct SyncChanges {
    /// この端末（始めた側）
    pub local: SyncDeviceChanges,
    /// 相手の端末
    pub remote: SyncDeviceChanges,
}

impl SyncChanges {
    pub fn is_empty(&self) -> bool {
        self.local.is_empty() && self.remote.is_empty()
    }
}

/// 突き合わせでまとめた ID の読み替え（片方の端末の分）。
#[derive(Debug, Default)]
pub(crate) struct IdMerges {
    /// 消す写し → (残す写し, 中身が同じか)
    pub(crate) matches: BTreeMap<MatchId, (MatchId, bool)>,
    pub(crate) teams: BTreeMap<TeamId, TeamId>,
    pub(crate) players: BTreeMap<PlayerId, PlayerId>,
}

impl IdMerges {
    fn team(&self, id: TeamId) -> TeamId {
        self.teams.get(&id).copied().unwrap_or(id)
    }

    fn player(&self, id: PlayerId) -> PlayerId {
        self.players.get(&id).copied().unwrap_or(id)
    }

    fn players(&self, ids: &BTreeSet<PlayerId>) -> BTreeSet<PlayerId> {
        ids.iter().map(|id| self.player(*id)).collect()
    }

    /// fact の参照（チーム・選手）を読み替えた写し。
    fn fact(&self, fact: &MatchFact) -> MatchFact {
        let mut fact = fact.clone();
        match &mut fact.payload {
            MatchFactPayload::Play(play) => {
                play.team_id = play.team_id.map(|id| self.team(id));
                play.player_id = play.player_id.map(|id| self.player(id));
                play.related_player_id = play.related_player_id.map(|id| self.player(id));
            }
            MatchFactPayload::Possession(possession) => {
                possession.team_id = self.team(possession.team_id);
            }
            MatchFactPayload::Control(_) => {}
        }
        fact
    }
}

/// `before`（この端末の今の全記録）を `after`（そろえた中身）にすると、何が変わるか。
/// どちらも時刻をミリ秒に丸めたもの（`normalize`）を渡す。
pub(crate) fn device_changes(
    before: &SyncSnapshot,
    after: &SyncSnapshot,
    merges: &IdMerges,
) -> SyncDeviceChanges {
    let (before, after) = (Live::new(before), Live::new(after));
    SyncDeviceChanges {
        matches: match_changes(&before, &after, merges),
        teams: team_changes(&before, &after, merges),
    }
}

/// 片方の全記録のうち、消されていないもの（fact は、消されていない試合のものだけ）。
struct Live<'a> {
    matches: BTreeMap<MatchId, &'a SyncMatch>,
    facts: BTreeMap<MatchId, BTreeMap<FactId, &'a SyncFact>>,
    teams: BTreeMap<TeamId, &'a SyncTeam>,
    players: Vec<&'a SyncPlayer>,
}

impl<'a> Live<'a> {
    fn new(snapshot: &'a SyncSnapshot) -> Live<'a> {
        let matches: BTreeMap<MatchId, &SyncMatch> = snapshot
            .matches
            .iter()
            .filter(|m| !m.stamp.is_deleted())
            .map(|m| (m.match_.id, m))
            .collect();
        let mut facts: BTreeMap<MatchId, BTreeMap<FactId, &SyncFact>> = BTreeMap::new();
        for f in &snapshot.facts {
            if !f.stamp.is_deleted() && matches.contains_key(&f.match_id) {
                facts.entry(f.match_id).or_default().insert(f.fact.id, f);
            }
        }
        Live {
            matches,
            facts,
            teams: snapshot
                .teams
                .iter()
                .filter(|t| !t.stamp.is_deleted())
                .map(|t| (t.team.id, t))
                .collect(),
            players: snapshot
                .players
                .iter()
                .filter(|p| !p.stamp.is_deleted())
                .collect(),
        }
    }

    fn facts_of(&self, id: MatchId) -> BTreeMap<FactId, &'a SyncFact> {
        self.facts.get(&id).cloned().unwrap_or_default()
    }
}

// ── 試合 ──

fn match_changes(before: &Live<'_>, after: &Live<'_>, merges: &IdMerges) -> Vec<SyncMatchChange> {
    // 並べるための日付と、行。
    let mut rows: Vec<(DateTime<Utc>, SyncMatchChange)> = Vec::new();

    // 写しをまとめた試合: この端末の写し（消す側）を、残す側の試合として 1 行にする。
    let mut replacing: BTreeSet<MatchId> = BTreeSet::new();
    for (loser, (survivor, identical)) in &merges.matches {
        let Some(own) = before.matches.get(loser) else {
            continue;
        };
        replacing.insert(*survivor);
        match after.matches.get(survivor) {
            Some(kept) if !identical => rows.push((kept.match_.date, copy_replaced(*survivor))),
            Some(_) => {}
            None => rows.push((own.match_.date, removed_match(*loser))),
        }
    }

    let ids: BTreeSet<MatchId> = before
        .matches
        .keys()
        .chain(after.matches.keys())
        .copied()
        .collect();
    for id in ids {
        if merges.matches.contains_key(&id)
            || (replacing.contains(&id) && !before.matches.contains_key(&id))
        {
            continue;
        }
        match (before.matches.get(&id), after.matches.get(&id)) {
            (None, Some(added)) => rows.push((
                added.match_.date,
                SyncMatchChange {
                    facts_added: count(after.facts_of(id).len()),
                    ..empty_match_change(id, SyncChangeKind::Added)
                },
            )),
            (Some(own), None) => rows.push((own.match_.date, removed_match(id))),
            (Some(own), Some(merged)) => {
                if let Some(change) = updated_match(
                    own,
                    merged,
                    &before.facts_of(id),
                    &after.facts_of(id),
                    merges,
                ) {
                    rows.push((merged.match_.date, change));
                }
            }
            (None, None) => {}
        }
    }

    rows.sort_by_key(|(date, change)| (Reverse(*date), change.match_id));
    rows.into_iter().map(|(_, change)| change).collect()
}

fn updated_match(
    own: &SyncMatch,
    merged: &SyncMatch,
    own_facts: &BTreeMap<FactId, &SyncFact>,
    merged_facts: &BTreeMap<FactId, &SyncFact>,
    merges: &IdMerges,
) -> Option<SyncMatchChange> {
    let (before, after) = (&own.match_, &merged.match_);
    let mut fields: Vec<SyncMatchField> = Vec::new();
    if before.title != after.title {
        fields.push(SyncMatchField::Title);
    }
    if before.date != after.date {
        fields.push(SyncMatchField::Date);
    }
    if merges.team(before.home_team_id) != after.home_team_id
        || merges.team(before.away_team_id) != after.away_team_id
    {
        fields.push(SyncMatchField::Teams);
    }
    if !configuration_equal(
        &before.configuration,
        own.local_video.as_ref(),
        &after.configuration,
        merged.local_video.as_ref(),
    ) {
        fields.push(SyncMatchField::Video);
    }
    let (own_roster, merged_roster) = (&before.roster_selection, &after.roster_selection);
    if merges.players(&own_roster.benched_player_ids) != merged_roster.benched_player_ids
        || merges.players(&own_roster.out_of_roster_player_ids)
            != merged_roster.out_of_roster_player_ids
    {
        fields.push(SyncMatchField::Roster);
    }

    let facts_added = merged_facts
        .keys()
        .filter(|id| !own_facts.contains_key(*id))
        .count();
    let facts_removed = own_facts
        .keys()
        .filter(|id| !merged_facts.contains_key(*id))
        .count();
    let facts_changed = own_facts
        .iter()
        .filter(|(id, own)| {
            merged_facts
                .get(*id)
                .is_some_and(|merged| merges.fact(&own.fact) != merged.fact)
        })
        .count();

    if fields.is_empty() && facts_added == 0 && facts_removed == 0 && facts_changed == 0 {
        return None;
    }
    Some(SyncMatchChange {
        fields,
        facts_added: count(facts_added),
        facts_removed: count(facts_removed),
        facts_changed: count(facts_changed),
        ..empty_match_change(after.id, SyncChangeKind::Updated)
    })
}

fn empty_match_change(id: MatchId, kind: SyncChangeKind) -> SyncMatchChange {
    SyncMatchChange {
        match_id: id,
        kind,
        fields: Vec::new(),
        facts_added: 0,
        facts_removed: 0,
        facts_changed: 0,
        copy_replaced: false,
    }
}

fn removed_match(id: MatchId) -> SyncMatchChange {
    empty_match_change(id, SyncChangeKind::Removed)
}

fn copy_replaced(survivor: MatchId) -> SyncMatchChange {
    SyncMatchChange {
        copy_replaced: true,
        ..empty_match_change(survivor, SyncChangeKind::Updated)
    }
}

// ── チーム ──

fn team_changes(before: &Live<'_>, after: &Live<'_>, merges: &IdMerges) -> Vec<SyncTeamChange> {
    // この端末のチームを、まとめた先の ID ごとにまとめる（突き合わせで 1 つにしたチーム）。
    let mut own_teams: BTreeMap<TeamId, Vec<&SyncTeam>> = BTreeMap::new();
    for team in before.teams.values().copied() {
        own_teams
            .entry(merges.team(team.team.id))
            .or_default()
            .push(team);
    }
    let own_players = players_by_team(before, merges);
    let merged_players = players_by_team(after, &IdMerges::default());
    let no_players = BTreeMap::new();

    let ids: BTreeSet<TeamId> = own_teams
        .keys()
        .chain(after.teams.keys())
        .copied()
        .collect();
    // 並べるための名前と、行。
    let mut rows: Vec<(&str, SyncTeamChange)> = Vec::new();
    for id in ids {
        let mine = own_players.get(&id).unwrap_or(&no_players);
        let theirs = merged_players.get(&id).unwrap_or(&no_players);
        match (own_teams.get(&id), after.teams.get(&id)) {
            (None, Some(added)) => rows.push((
                added.team.name.as_str(),
                SyncTeamChange {
                    players_added: count(theirs.len()),
                    ..empty_team_change(id, SyncChangeKind::Added)
                },
            )),
            (Some(group), None) => {
                let own = group.iter().find(|t| t.team.id == id).unwrap_or(&group[0]);
                rows.push((
                    own.team.name.as_str(),
                    empty_team_change(own.team.id, SyncChangeKind::Removed),
                ));
            }
            (Some(group), Some(merged)) => {
                // まとめた先のチームがこの端末に無ければ、名前の同じチームをまとめたので名前は変わらない。
                let renamed = group
                    .iter()
                    .find(|t| t.team.id == id)
                    .is_some_and(|own| own.team.name != merged.team.name);
                let players_added = theirs.keys().filter(|p| !mine.contains_key(*p)).count();
                let players_removed = mine.keys().filter(|p| !theirs.contains_key(*p)).count();
                let players_changed = mine
                    .iter()
                    .filter(|(p, own)| {
                        theirs.get(*p).is_some_and(|merged| {
                            own.player.name != merged.player.name
                                || own.player.jersey_number != merged.player.jersey_number
                        })
                    })
                    .count();
                if renamed || players_added > 0 || players_removed > 0 || players_changed > 0 {
                    rows.push((
                        merged.team.name.as_str(),
                        SyncTeamChange {
                            renamed,
                            players_added: count(players_added),
                            players_removed: count(players_removed),
                            players_changed: count(players_changed),
                            ..empty_team_change(id, SyncChangeKind::Updated)
                        },
                    ));
                }
            }
            (None, None) => {}
        }
    }

    rows.sort_by(|(a_name, a), (b_name, b)| a_name.cmp(b_name).then(a.team_id.cmp(&b.team_id)));
    rows.into_iter().map(|(_, change)| change).collect()
}

/// チーム → 選手（どちらの ID も読み替えた後）。
fn players_by_team<'a>(
    live: &Live<'a>,
    merges: &IdMerges,
) -> BTreeMap<TeamId, BTreeMap<PlayerId, &'a SyncPlayer>> {
    let mut players: BTreeMap<TeamId, BTreeMap<PlayerId, &SyncPlayer>> = BTreeMap::new();
    for p in live.players.iter().copied() {
        players
            .entry(merges.team(p.player.team_id))
            .or_default()
            .insert(merges.player(p.player.id), p);
    }
    players
}

fn empty_team_change(id: TeamId, kind: SyncChangeKind) -> SyncTeamChange {
    SyncTeamChange {
        team_id: id,
        kind,
        renamed: false,
        players_added: 0,
        players_removed: 0,
        players_changed: 0,
    }
}

/// 数を FFI の型へ（一端末の記録の数は `u32` に収まる）。
fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
