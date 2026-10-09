//! 同期の保存入口（発火層 `ffi_write::apply_sync` — ADR 0007 決定 1）の挙動固定。
//!
//! - 店の全記録を、届いた中身と同じにする（届いた中身に無い記録は消える）
//! - 端末ごとの値（左右配置・写真）は、保存の直前に読んだ店の値を残す
//! - repository の失敗はそのまま返し、店を変えない
//!
//! async 入口は pollster で回す（fake は即時完了 future のためランタイム不要）。

use std::future::Future;
use std::sync::{Arc, Mutex};

use handball_toolkit::configuration::MatchConfiguration;
use handball_toolkit::entities::{Match, Player, PlayerPhoto, RosterSelection, Team};
use handball_toolkit::ids::{MatchId, PlayerId, TeamId};
use handball_toolkit::sync::{SyncMatch, SyncPlayer, SyncSnapshot, SyncStamp, SyncTeam};
use handball_toolkit_ffi::ffi_write::{CoreWriteError, SyncWriteRepository, apply_sync};
use uuid::Uuid;

fn run<F: Future>(future: F) -> F::Output {
    pollster::block_on(future)
}

#[test]
fn apply_sync_replaces_the_store_and_keeps_this_devices_values() {
    let mut stored = snapshot();
    stored.matches[0].match_.is_home_on_left = false;
    stored.players[0].player.photo = Some(PlayerPhoto {
        storage_key: "alice.jpg".to_owned(),
    });
    let only_here = Team {
        id: TeamId(Uuid::from_u128(9)),
        name: "この端末だけのチーム".to_owned(),
    };
    stored.teams.push(SyncTeam {
        team: only_here.clone(),
        stamp: stamp(),
    });
    let repo = Arc::new(FakeSyncRepo::new(stored));

    let mut incoming = snapshot();
    incoming.matches[0].match_.title = Some("決勝".to_owned());
    let relinks = run(apply_sync(repo.clone(), incoming)).expect("保存できる");

    let saved = repo.stored();
    assert!(relinks.is_empty());
    assert_eq!(saved.matches[0].match_.title.as_deref(), Some("決勝"));
    assert!(!saved.matches[0].match_.is_home_on_left);
    assert_eq!(
        saved.players[0]
            .player
            .photo
            .as_ref()
            .map(|p| p.storage_key.as_str()),
        Some("alice.jpg")
    );
    assert!(!saved.teams.iter().any(|t| t.team.id == only_here.id));
}

#[test]
fn apply_sync_leaves_the_store_unchanged_when_replacing_fails() {
    let stored = snapshot();
    let repo = Arc::new(FakeSyncRepo {
        stored: Mutex::new(stored.clone()),
        fail_replace: true,
    });

    let mut incoming = snapshot();
    incoming.matches[0].match_.title = Some("決勝".to_owned());
    let result = run(apply_sync(repo.clone(), incoming));

    assert!(matches!(result, Err(CoreWriteError::Repository { .. })));
    assert_eq!(repo.stored(), stored);
}

// ── helpers ──

#[derive(Debug)]
struct FakeSyncRepo {
    stored: Mutex<SyncSnapshot>,
    fail_replace: bool,
}

impl FakeSyncRepo {
    fn new(stored: SyncSnapshot) -> FakeSyncRepo {
        FakeSyncRepo {
            stored: Mutex::new(stored),
            fail_replace: false,
        }
    }

    fn stored(&self) -> SyncSnapshot {
        self.stored
            .lock()
            .expect("テスト内で poison しない")
            .clone()
    }
}

#[async_trait::async_trait]
impl SyncWriteRepository for FakeSyncRepo {
    async fn load_sync_snapshot(&self) -> Result<SyncSnapshot, CoreWriteError> {
        Ok(self.stored())
    }

    async fn replace_all(&self, snapshot: SyncSnapshot) -> Result<(), CoreWriteError> {
        if self.fail_replace {
            return Err(CoreWriteError::Repository {
                detail: "書けなかった".to_owned(),
            });
        }
        *self.stored.lock().expect("テスト内で poison しない") = snapshot;
        Ok(())
    }
}

fn stamp() -> SyncStamp {
    SyncStamp {
        updated_at: chrono::DateTime::from_timestamp(1, 0).expect("有効な時刻"),
        deleted_at: None,
    }
}

/// チーム 2・選手 1・タイマーの試合 1（fact なし）。
fn snapshot() -> SyncSnapshot {
    let home = TeamId(Uuid::from_u128(1));
    let away = TeamId(Uuid::from_u128(2));
    SyncSnapshot {
        matches: vec![SyncMatch {
            match_: Match {
                id: MatchId(Uuid::from_u128(10)),
                title: None,
                date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
                home_team_id: home,
                away_team_id: away,
                configuration: MatchConfiguration::Timer {
                    phase_duration_seconds: 1800.0,
                },
                roster_selection: RosterSelection::default(),
                is_home_on_left: true,
            },
            stamp: stamp(),
            local_video: None,
        }],
        teams: vec![
            SyncTeam {
                team: Team {
                    id: home,
                    name: "Home".to_owned(),
                },
                stamp: stamp(),
            },
            SyncTeam {
                team: Team {
                    id: away,
                    name: "Away".to_owned(),
                },
                stamp: stamp(),
            },
        ],
        players: vec![SyncPlayer {
            player: Player {
                id: PlayerId(Uuid::from_u128(3)),
                team_id: home,
                name: "Alice".to_owned(),
                jersey_number: Some(7),
                photo: None,
            },
            stamp: stamp(),
        }],
        facts: vec![],
    }
}
