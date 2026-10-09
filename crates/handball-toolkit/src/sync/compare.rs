//! 同期で比べる中身（ADR 0007 決定 2）。端末ごとの値は比べない。
//!
//! | 記録 | 比べない値 | 理由 |
//! |---|---|---|
//! | 試合 | `is_home_on_left` | スコア・イベント一覧の左右配置。見方の設定として端末ごとに持つ |
//! | 試合 | 端末内動画の `external_id` | 端末ごとに違う参照。同じ動画かは `cloud_identifier` で見る |
//! | 選手 | `photo` | 写真は端末ごとに持ち、同期では運ばない |
//!
//! シェルの永続化も、これらだけを書き換えたときは `updated_at` を進めない（進めると、別の端末で
//! 直した中身がそれに負けて戻る）。

use crate::configuration::{MatchConfiguration, VideoProvider};

use super::{LocalVideoIdentity, SyncFact, SyncMatch, SyncPlayer, SyncTeam};

/// 試合の中身が同じか（`is_home_on_left` と端末内動画の参照を除く）。
pub fn match_content_equal(a: &SyncMatch, b: &SyncMatch) -> bool {
    let (x, y) = (&a.match_, &b.match_);
    x.id == y.id
        && x.title == y.title
        && x.date == y.date
        && x.home_team_id == y.home_team_id
        && x.away_team_id == y.away_team_id
        && x.roster_selection == y.roster_selection
        && configuration_equal(
            &x.configuration,
            a.local_video.as_ref(),
            &y.configuration,
            b.local_video.as_ref(),
        )
}

pub(crate) fn team_content_equal(a: &SyncTeam, b: &SyncTeam) -> bool {
    a.team.id == b.team.id && a.team.name == b.team.name
}

/// 選手の中身が同じか（写真を除く）。
pub fn player_content_equal(a: &SyncPlayer, b: &SyncPlayer) -> bool {
    let (x, y) = (&a.player, &b.player);
    x.id == y.id && x.team_id == y.team_id && x.name == y.name && x.jersey_number == y.jersey_number
}

/// fact の中身が同じか（すべての値を比べる）。
pub fn fact_content_equal(a: &SyncFact, b: &SyncFact) -> bool {
    a.match_id == b.match_id && a.fact == b.fact
}

fn configuration_equal(
    a: &MatchConfiguration,
    a_local: Option<&LocalVideoIdentity>,
    b: &MatchConfiguration,
    b_local: Option<&LocalVideoIdentity>,
) -> bool {
    match (a, b) {
        (MatchConfiguration::Video(source_a), MatchConfiguration::Video(source_b))
        | (
            MatchConfiguration::VideoHighlight(source_a),
            MatchConfiguration::VideoHighlight(source_b),
        ) if source_a.provider == VideoProvider::Local
            && source_b.provider == VideoProvider::Local =>
        {
            same_local_video(a_local, b_local)
        }
        _ => a == b,
    }
}

/// 端末内動画どうしが同じ動画か。**どちらかの `cloud_identifier` が取れていなければ同じとみなす**
/// — 比べる手がかりが無いので、その端末の参照を残す側に倒す（ADR 0007 決定 2）。
pub(crate) fn same_local_video(
    a: Option<&LocalVideoIdentity>,
    b: Option<&LocalVideoIdentity>,
) -> bool {
    fn cloud(video: Option<&LocalVideoIdentity>) -> Option<&str> {
        video.and_then(|v| v.cloud_identifier.as_deref())
    }
    match (cloud(a), cloud(b)) {
        (Some(x), Some(y)) => x == y,
        _ => true,
    }
}
