//! 試合をまとめて 1 つのファイルで運ぶ（まとめファイル — handball-project#669。判断の記録は ADR 0007
//! の「実装追記」）。
//!
//! 中身は同期で運ぶ形（[`SyncPayload`]）そのもので、取り込む側は同期と同じ [`super::reconcile`] で
//! この端末の全記録とそろえる（ファイルを相手の端末とみなす）。同期と違うのは 2 つ:
//!
//! - **選んだ試合だけを入れられる**（[`select_sync_matches`]）。そろえるは「片方に無い」を「消した」と
//!   みなさない（消したことは削除の記録で運ぶ）ので、一部の試合だけのファイルでも取り込める
//! - **古い版のファイルを読み続ける**（[`decode_sync_file`]）。同期は両方の端末が同じ版のときにしか
//!   起きないが、ファイルはバックアップとして端末の外に残り、アプリを更新した後に開かれる

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::facts::MatchFactPayload;
use crate::ids::{MatchId, PlayerId, TeamId};

use super::payload::invalid_json;
use super::{SYNC_FORMAT_VERSION, SyncPayload, SyncPayloadError, SyncSnapshot};

/// ファイルとして読める最も古い版。**上げない**（上げると、その版で書いたバックアップが読めなくなる）。
pub const SYNC_FILE_OLDEST_FORMAT_VERSION: u32 = 1;

/// 全記録から、選んだ試合と、それを読むのに要る記録だけを取り出す。
///
/// - 試合: `match_ids` のうち消されていないもの（消した試合は選べない）
/// - fact: その試合の全部（消した fact も — 消したことを運ぶ）
/// - チーム: その試合のホームとアウェイ、fact と出場の選択が指す選手とチーム
/// - 選手: そのチームの全員（消した選手も）
///
/// 入れなかった試合・チーム・選手と、その削除の記録は運ばない。取り込む側では「片方に無い」になり、
/// その端末の記録がそのまま残る。全記録を送るときはこの関数を通さず、全記録をそのまま運ぶ
/// （試合に使われていないチームと、消した試合の記録も入る）。
pub fn select_sync_matches(snapshot: &SyncSnapshot, match_ids: &[MatchId]) -> SyncSnapshot {
    let wanted: BTreeSet<MatchId> = match_ids.iter().copied().collect();
    let matches: Vec<_> = snapshot
        .matches
        .iter()
        .filter(|m| wanted.contains(&m.match_.id) && !m.stamp.is_deleted())
        .cloned()
        .collect();
    let chosen: BTreeSet<MatchId> = matches.iter().map(|m| m.match_.id).collect();
    let facts: Vec<_> = snapshot
        .facts
        .iter()
        .filter(|f| chosen.contains(&f.match_id))
        .cloned()
        .collect();

    let mut teams: BTreeSet<TeamId> = BTreeSet::new();
    let mut players: BTreeSet<PlayerId> = BTreeSet::new();
    for m in &matches {
        teams.insert(m.match_.home_team_id);
        teams.insert(m.match_.away_team_id);
        let roster = &m.match_.roster_selection;
        players.extend(roster.benched_player_ids.iter().copied());
        players.extend(roster.out_of_roster_player_ids.iter().copied());
    }
    for f in &facts {
        match &f.fact.payload {
            MatchFactPayload::Play(play) => {
                teams.extend(play.team_id);
                players.extend(play.player_id);
                players.extend(play.related_player_id);
            }
            MatchFactPayload::Possession(possession) => {
                teams.insert(possession.team_id);
            }
            MatchFactPayload::Control(_) => {}
        }
    }
    // 指された選手が別のチームにいても、そのチームごと運ぶ（取り込む側で選手の行き先が無くならない）。
    teams.extend(
        snapshot
            .players
            .iter()
            .filter(|p| players.contains(&p.player.id))
            .map(|p| p.player.team_id),
    );

    SyncSnapshot {
        matches,
        teams: snapshot
            .teams
            .iter()
            .filter(|t| teams.contains(&t.team.id))
            .cloned()
            .collect(),
        players: snapshot
            .players
            .iter()
            .filter(|p| teams.contains(&p.player.team_id))
            .cloned()
            .collect(),
        facts,
    }
}

/// まとめファイルの JSON を読む。
///
/// 同期の [`super::decode_sync_payload`] と違い、**今より古い版も読む**。今より新しい版は中身を読まずに
/// [`SyncPayloadError::UnsupportedFormatVersion`] を返す（シェルは「この端末のアプリを更新してください」と
/// 出せる）。
///
/// **`SYNC_FORMAT_VERSION` を上げるときは、ここに古い版の形を読んで今の形へ直す道を足す。** 足さずに
/// 上げると、古い版のバックアップが今の形として読まれて落ちる（`tests/sync_file_tests.rs` が、出した版の
/// ファイルを読めることを固定している）。
pub fn decode_sync_file(json: &str) -> Result<SyncPayload, SyncPayloadError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        format_version: u32,
    }

    let header: Header = serde_json::from_str(json).map_err(invalid_json)?;
    if !(SYNC_FILE_OLDEST_FORMAT_VERSION..=SYNC_FORMAT_VERSION).contains(&header.format_version) {
        return Err(SyncPayloadError::UnsupportedFormatVersion {
            found: header.format_version,
            supported: SYNC_FORMAT_VERSION,
        });
    }
    // 今は版 1 しか無いので、今の形として読む。
    serde_json::from_str(json).map_err(invalid_json)
}
