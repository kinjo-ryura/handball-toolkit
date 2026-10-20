//! まとめファイル（`sync::select_sync_matches` / `sync::decode_sync_file`。handball-project#669 / ADR 0007 実装追記）。
//!
//! 固定する挙動:
//! - 選んだ試合だけを取り出すときは、その試合の fact（消したものも）・ホームとアウェイ・指された選手の
//!   チームと、そのチームの選手（消したものも）を運ぶ。選ばなかった試合・消した試合・使われていない
//!   チームは運ばない
//! - 取り込みは同期と同じそろえる。一部の試合だけのファイルでも、この端末にしか無い記録は残る。
//!   空の端末に全記録を戻すと、聞かずにすべて入る。同じファイルをもう一度取り込んでも変わらない
//! - 出した版のファイルを読み続ける（`tests/fixtures/sync-files/`）。今より新しい版は中身を読まない

mod fixtures;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use handball_toolkit::configuration::{MatchConfiguration, VideoProvider, VideoSource};
use handball_toolkit::entities::{Match, Player, Team};
use handball_toolkit::facts::{MatchFact, MatchFactPayload, PlayEventKind};
use handball_toolkit::ids::{FactId, MatchId, PlayerId, TeamId};
use handball_toolkit::sync::{
    LocalVideoIdentity, SYNC_FORMAT_VERSION, SyncFact, SyncMatch, SyncPayload, SyncPayloadError,
    SyncPlayer, SyncReconcileResult, SyncSnapshot, SyncStamp, SyncTeam, decode_sync_file,
    encode_sync_payload, reconcile, select_sync_matches,
};
use uuid::Uuid;

use fixtures::{epoch, make_timer_match, make_video_match, play_at_match, timer_phase};

// ── 選んだ試合だけを取り出す ──

#[test]
fn selected_match_carries_its_facts_teams_and_players() {
    let d = Device::new();

    let selected = select_sync_matches(&d.snapshot(), &[d.timer_match]);

    assert_eq!(match_ids(&selected), vec![d.timer_match]);
    // 消した fact も運ぶ（消したことが取り込む側に届く）。
    assert_eq!(
        fact_ids(&selected),
        set([d.timer_phase, d.timer_goal, d.timer_goal_removed])
    );
    assert_eq!(team_ids(&selected), set([d.home, d.away]));
    // そのチームの選手は全員。消した選手も。
    assert_eq!(
        player_ids(&selected),
        set([d.home_scorer, d.home_player_removed, d.away_player])
    );
}

#[test]
fn unselected_deleted_and_unused_records_are_left_out() {
    let d = Device::new();

    let selected = select_sync_matches(&d.snapshot(), &[d.timer_match, d.deleted_match]);

    // 消した試合は選べない。選ばなかった試合のチーム（visitor）と、どの試合にも使われていない
    // チーム（unused）も運ばない。
    assert_eq!(match_ids(&selected), vec![d.timer_match]);
    assert!(!team_ids(&selected).contains(&d.visitor));
    assert!(!team_ids(&selected).contains(&d.unused));
    assert!(!player_ids(&selected).contains(&d.visitor_player));
}

#[test]
fn player_from_another_team_brings_that_team_along() {
    let d = Device::new();
    let mut snapshot = d.snapshot();
    // 得点のアシストに、試合の外のチーム（visitor）の選手を指す。
    let goal = snapshot
        .facts
        .iter_mut()
        .find(|f| f.fact.id == d.timer_goal)
        .unwrap();
    if let MatchFactPayload::Play(play) = &mut goal.fact.payload {
        play.related_player_id = Some(d.visitor_player);
    }

    let selected = select_sync_matches(&snapshot, &[d.timer_match]);

    assert!(team_ids(&selected).contains(&d.visitor));
    assert!(player_ids(&selected).contains(&d.visitor_player));
}

// ── そろえるで取り込む ──

#[test]
fn file_with_some_matches_keeps_what_only_this_device_has() {
    let d = Device::new();
    let mut sent = select_sync_matches(&d.snapshot(), &[d.timer_match]);
    for m in &mut sent.matches {
        m.match_.title = Some("直したタイトル".to_owned());
        m.stamp = alive(100);
    }

    let result = merged(reconcile(&d.snapshot(), &sent, &[], at(200)));

    // ファイルに無い試合（video_match）は消えず、ファイルの直しは入る。
    assert_eq!(live_match_ids(&result), set([d.timer_match, d.video_match]));
    let title = result
        .matches
        .iter()
        .find(|m| m.match_.id == d.timer_match)
        .and_then(|m| m.match_.title.clone());
    assert_eq!(title.as_deref(), Some("直したタイトル"));
}

#[test]
fn everything_restored_into_an_empty_device_without_asking() {
    let d = Device::new();
    let backup = d.snapshot();

    let result = reconcile(&SyncSnapshot::default(), &backup, &[], at(200));

    let SyncReconcileResult::Merged {
        snapshot, changes, ..
    } = result
    else {
        panic!("問いが出た: {result:?}");
    };
    // 消した記録も含めて、全部がそのまま入る（後で元の端末と同期しても二重にならない）。
    assert_eq!(match_ids(&snapshot), match_ids(&backup));
    assert_eq!(fact_ids(&snapshot), fact_ids(&backup));
    assert_eq!(team_ids(&snapshot), team_ids(&backup));
    assert_eq!(player_ids(&snapshot), player_ids(&backup));
    let deleted = snapshot
        .matches
        .iter()
        .find(|m| m.match_.id == d.deleted_match)
        .unwrap();
    assert!(deleted.stamp.is_deleted());
    assert!(!changes.local.is_empty());
}

#[test]
fn importing_the_same_file_again_changes_nothing() {
    let d = Device::new();
    let backup = d.snapshot();
    let restored = merged(reconcile(&SyncSnapshot::default(), &backup, &[], at(200)));

    let again = reconcile(&restored, &backup, &[], at(300));

    let SyncReconcileResult::Merged { changes, .. } = again else {
        panic!("問いが出た: {again:?}");
    };
    assert!(changes.local.is_empty());
}

// ── 版 ──

/// 出した版のまとめファイルを読み続ける約束。ここが落ちたら、古い版のバックアップが読めなくなる変更を
/// 入れている。直すのは fixture ではなくコード（版を上げて、古い版を今の形へ直す道を `decode_sync_file` に
/// 足す）。
#[test]
fn sync_file_of_every_released_version_still_reads() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sync-files");
    let json = fs::read_to_string(dir.join("sync-file-v1.json")).unwrap();

    let payload = decode_sync_file(&json).unwrap();

    assert_eq!(payload.format_version, 1);
    assert_eq!(payload.device_id, Uuid::from_u128(99));
    assert_eq!(payload.snapshot, Device::new().snapshot());
}

#[test]
fn sync_file_round_trips_exactly() {
    let payload = SyncPayload {
        format_version: SYNC_FORMAT_VERSION,
        device_id: Uuid::from_u128(99),
        snapshot: Device::new().snapshot(),
    };

    let json = encode_sync_payload(&payload).unwrap();

    assert_eq!(decode_sync_file(&json).unwrap(), payload);
}

#[test]
fn sync_file_from_a_newer_app_is_rejected_before_reading_it() {
    let json = format!(
        r#"{{"formatVersion": {}, "somethingNew": true}}"#,
        SYNC_FORMAT_VERSION + 1
    );

    assert_eq!(
        decode_sync_file(&json),
        Err(SyncPayloadError::UnsupportedFormatVersion {
            found: SYNC_FORMAT_VERSION + 1,
            supported: SYNC_FORMAT_VERSION,
        })
    );
}

#[test]
fn sync_file_that_is_not_json_is_invalid() {
    assert!(matches!(
        decode_sync_file("not json"),
        Err(SyncPayloadError::InvalidJson { .. })
    ));
}

// ── helpers ──

/// 1 台の端末の全記録:
/// - タイマーの試合（home vs away）: 区切り・得点・消した得点
/// - 端末内動画の試合（visitor vs away）
/// - 消した試合（home vs away）
/// - チーム 4 つ（unused はどの試合にも使われていない）・選手 4 人（home の 1 人は消してある）
struct Device {
    home: TeamId,
    away: TeamId,
    visitor: TeamId,
    unused: TeamId,
    home_scorer: PlayerId,
    home_player_removed: PlayerId,
    away_player: PlayerId,
    visitor_player: PlayerId,
    timer_match: MatchId,
    video_match: MatchId,
    deleted_match: MatchId,
    timer_phase: FactId,
    timer_goal: FactId,
    timer_goal_removed: FactId,
    video_phase: FactId,
}

impl Device {
    fn new() -> Device {
        Device {
            home: TeamId(Uuid::from_u128(1)),
            away: TeamId(Uuid::from_u128(2)),
            visitor: TeamId(Uuid::from_u128(3)),
            unused: TeamId(Uuid::from_u128(4)),
            home_scorer: PlayerId(Uuid::from_u128(11)),
            home_player_removed: PlayerId(Uuid::from_u128(12)),
            away_player: PlayerId(Uuid::from_u128(13)),
            visitor_player: PlayerId(Uuid::from_u128(14)),
            timer_match: MatchId(Uuid::from_u128(21)),
            video_match: MatchId(Uuid::from_u128(22)),
            deleted_match: MatchId(Uuid::from_u128(23)),
            timer_phase: FactId(Uuid::from_u128(31)),
            timer_goal: FactId(Uuid::from_u128(32)),
            timer_goal_removed: FactId(Uuid::from_u128(33)),
            video_phase: FactId(Uuid::from_u128(34)),
        }
    }

    fn snapshot(&self) -> SyncSnapshot {
        let mut timer = make_timer_match(self.home, self.away);
        timer.id = self.timer_match;
        timer.title = Some("練習試合".to_owned());
        let mut video = make_video_match(self.visitor, self.away);
        video.id = self.video_match;
        video.configuration = MatchConfiguration::Video(VideoSource {
            provider: VideoProvider::Local,
            external_id: "ABCD-1234/L0/001".to_owned(),
        });
        let mut removed = make_timer_match(self.home, self.away);
        removed.id = self.deleted_match;

        let mut goal = play_at_match(PlayEventKind::Goal, self.home, self.home_scorer, 600.0);
        goal.id = self.timer_goal;
        goal.recorded_at = at_ms(600_123);
        let mut goal_removed =
            play_at_match(PlayEventKind::Goal, self.away, self.away_player, 900.5);
        goal_removed.id = self.timer_goal_removed;
        goal_removed.recorded_at = at_ms(900_456);

        SyncSnapshot {
            matches: vec![
                sync_match(timer, alive(10), None),
                sync_match(
                    video,
                    alive(20),
                    Some(LocalVideoIdentity {
                        cloud_identifier: Some("cloud-1".to_owned()),
                        duration_seconds: Some(3725.25),
                    }),
                ),
                sync_match(removed, deleted(30), None),
            ],
            teams: vec![
                team(self.home, "ホーム", alive(1)),
                team(self.away, "アウェイ", alive(1)),
                team(self.visitor, "ビジター", alive(1)),
                team(self.unused, "名簿だけ", alive(1)),
            ],
            players: vec![
                player(self.home_scorer, self.home, "選手A", Some(7), alive(2)),
                player(
                    self.home_player_removed,
                    self.home,
                    "選手B",
                    None,
                    deleted(40),
                ),
                player(self.away_player, self.away, "選手C", Some(10), alive(2)),
                player(
                    self.visitor_player,
                    self.visitor,
                    "選手D",
                    Some(4),
                    alive(2),
                ),
            ],
            facts: vec![
                fact(
                    self.timer_match,
                    phase(self.timer_phase, at_ms(1_001)),
                    alive(10),
                ),
                fact(self.timer_match, goal, alive(11)),
                fact(self.timer_match, goal_removed, deleted(50)),
                fact(
                    self.video_match,
                    phase(self.video_phase, at_ms(2_002)),
                    alive(20),
                ),
            ],
        }
    }
}

fn phase(id: FactId, recorded_at: DateTime<Utc>) -> MatchFact {
    let mut fact = timer_phase(id, 0.0, 1800.0);
    fact.recorded_at = recorded_at;
    fact
}

fn sync_match(
    match_: Match,
    stamp: SyncStamp,
    local_video: Option<LocalVideoIdentity>,
) -> SyncMatch {
    SyncMatch {
        match_,
        stamp,
        local_video,
    }
}

fn team(id: TeamId, name: &str, stamp: SyncStamp) -> SyncTeam {
    SyncTeam {
        team: Team {
            id,
            name: name.to_owned(),
        },
        stamp,
    }
}

fn player(
    id: PlayerId,
    team_id: TeamId,
    name: &str,
    jersey_number: Option<i64>,
    stamp: SyncStamp,
) -> SyncPlayer {
    SyncPlayer {
        player: Player {
            id,
            team_id,
            name: name.to_owned(),
            jersey_number,
            photo: None,
        },
        stamp,
    }
}

fn fact(match_id: MatchId, fact: MatchFact, stamp: SyncStamp) -> SyncFact {
    SyncFact {
        match_id,
        fact,
        stamp,
    }
}

fn set<T: Ord, const N: usize>(items: [T; N]) -> BTreeSet<T> {
    items.into_iter().collect()
}

fn match_ids(snapshot: &SyncSnapshot) -> Vec<MatchId> {
    snapshot.matches.iter().map(|m| m.match_.id).collect()
}

fn live_match_ids(snapshot: &SyncSnapshot) -> BTreeSet<MatchId> {
    snapshot
        .matches
        .iter()
        .filter(|m| !m.stamp.is_deleted())
        .map(|m| m.match_.id)
        .collect()
}

fn fact_ids(snapshot: &SyncSnapshot) -> BTreeSet<FactId> {
    snapshot.facts.iter().map(|f| f.fact.id).collect()
}

fn team_ids(snapshot: &SyncSnapshot) -> BTreeSet<TeamId> {
    snapshot.teams.iter().map(|t| t.team.id).collect()
}

fn player_ids(snapshot: &SyncSnapshot) -> BTreeSet<PlayerId> {
    snapshot.players.iter().map(|p| p.player.id).collect()
}

fn at(secs: i64) -> DateTime<Utc> {
    epoch() + TimeDelta::seconds(secs)
}

fn at_ms(millis: i64) -> DateTime<Utc> {
    epoch() + TimeDelta::milliseconds(millis)
}

fn alive(secs: i64) -> SyncStamp {
    SyncStamp {
        updated_at: at(secs),
        deleted_at: None,
    }
}

fn deleted(secs: i64) -> SyncStamp {
    SyncStamp {
        updated_at: at(secs),
        deleted_at: Some(at(secs)),
    }
}

fn merged(result: SyncReconcileResult) -> SyncSnapshot {
    match result {
        SyncReconcileResult::Merged { snapshot, .. } => snapshot,
        SyncReconcileResult::Questions { questions } => panic!("問いが出た: {questions:?}"),
    }
}

/// fixture を書き出す（版を上げて新しい版の fixture を足すときだけ、`--ignored` で 1 回回す）。
#[test]
#[ignore]
fn write_sync_file_fixture() {
    let payload = SyncPayload {
        format_version: SYNC_FORMAT_VERSION,
        device_id: Uuid::from_u128(99),
        snapshot: Device::new().snapshot(),
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sync-files");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("sync-file-v{SYNC_FORMAT_VERSION}.json")),
        encode_sync_payload(&payload).unwrap(),
    )
    .unwrap();
}
