//! write 入口（発火層 `ffi_write` — ADR 0005 実装順序 1）の挙動固定。
//!
//! fake repository で「読む → 検証 → 発火」の orchestration を FFI を越える前に固定する:
//! 合格時のみ発火・違反は `ValidationFailed` で不発火・repository 失敗の伝播・
//! roster 0 件 skip ルール（コアへ移した判断）。async 入口は pollster で回す
//! （fake は即時完了 future のためランタイム不要）。

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::entities::{Player, Team};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
};
use handball_toolkit::ids::{FactId, MatchId, PlayerId, TeamId};
use handball_toolkit::sample_dto::{
    SCHEMA_VERSION_CURRENT, SampleControlFactDtoV2, SampleFactAnchorDtoV2, SampleFactDtoV2,
    SampleFactPayloadDtoV2, SampleMatchClockDtoV2, SampleMatchConfigurationDtoV2, SampleMatchDtoV2,
    SampleMatchHeaderV2, SamplePhaseStartPayloadDtoV2, SamplePlayFactDtoV2, SamplePlayerDtoV2,
    SampleTeamDtoV2, SampleTeamsDtoV2, SampleTimerConfigurationDtoV2,
};
use handball_toolkit::sample_import::{ImportDecisions, ImportWriteBatch, TeamTarget};
use handball_toolkit::validation::{DomainValidationIssue, FactValidationError};
use handball_toolkit::write::{NewFactStamp, PlayerTeamRef, VideoSyncInput};
use handball_toolkit_ffi::ffi_write::{
    CoreWriteError, ImportWriteRepository, MatchWriteRepository, TeamWriteRepository,
    commit_sample_match_import, commit_video_migration, count_phase_completion_facts,
    record_append_fact, record_delete_fact, record_delete_player, record_delete_team,
    record_fact_with_phase_completion, record_save_player, record_save_team, record_update_fact,
};
use uuid::Uuid;

const MATCH_ID: u128 = 1;
const HOME_ID: u128 = 2;
const AWAY_ID: u128 = 3;
const PHASE_START_ID: u128 = 10;
const GOAL_ID: u128 = 11;
/// Goal kind は player 必須（`MissingPlayerForPlayKind`）。合格ケースはこれを渡す。
const SCORER_ID: u128 = 21;

fn run<F: Future>(future: F) -> F::Output {
    pollster::block_on(future)
}

fn timer_match() -> Match {
    Match {
        id: MatchId(Uuid::from_u128(MATCH_ID)),
        title: Some("write orchestration".to_string()),
        date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
        home_team_id: TeamId(Uuid::from_u128(HOME_ID)),
        away_team_id: TeamId(Uuid::from_u128(AWAY_ID)),
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    }
}

fn fact(id: u128, payload: MatchFactPayload) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: chrono::DateTime::from_timestamp(id as i64, 0).expect("固定秒は有効"),
        payload,
    }
}

fn phase_start() -> MatchFact {
    fact(
        PHASE_START_ID,
        MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: 0.0,
            }),
            end_anchor: FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: 1800.0,
            }),
        })),
    )
}

fn goal(id: u128, elapsed_seconds: f64, player_id: Option<PlayerId>) -> MatchFact {
    fact(
        id,
        MatchFactPayload::Play(PlayFact {
            kind: PlayEventKind::Goal,
            team_id: Some(TeamId(Uuid::from_u128(HOME_ID))),
            player_id,
            related_player_id: None,
            anchor: FactAnchor::MatchClock(MatchClock { elapsed_seconds }),
            title: None,
            note: None,
        }),
    )
}

/// 素朴 CRUD の in-memory fake。write 入口が「検証合格時のみ発火する」ことを
/// facts の変化で観測する。
#[derive(Debug)]
struct FakeRepo {
    match_: Match,
    facts: Mutex<Vec<MatchFact>>,
    roster_players: Vec<PlayerTeamRef>,
    fail_load_match: bool,
    saved_matches: Mutex<Vec<Match>>,
}

impl FakeRepo {
    fn new(facts: Vec<MatchFact>) -> Self {
        FakeRepo {
            match_: timer_match(),
            facts: Mutex::new(facts),
            roster_players: Vec::new(),
            fail_load_match: false,
            saved_matches: Mutex::new(Vec::new()),
        }
    }

    fn fact_log(&self) -> Vec<MatchFact> {
        self.facts.lock().expect("テスト内で poison しない").clone()
    }
}

#[async_trait::async_trait]
impl MatchWriteRepository for FakeRepo {
    async fn load_match(&self, _match_id: MatchId) -> Result<Match, CoreWriteError> {
        if self.fail_load_match {
            return Err(CoreWriteError::Repository {
                message: "load_match 失敗".to_string(),
            });
        }
        Ok(self.match_.clone())
    }

    async fn load_fact_log(&self, _match_id: MatchId) -> Result<Vec<MatchFact>, CoreWriteError> {
        Ok(self.fact_log())
    }

    async fn load_roster_players(
        &self,
        _home_team_id: TeamId,
        _away_team_id: TeamId,
    ) -> Result<Vec<PlayerTeamRef>, CoreWriteError> {
        Ok(self.roster_players.clone())
    }

    async fn save_match(&self, match_: Match) -> Result<(), CoreWriteError> {
        self.saved_matches
            .lock()
            .expect("テスト内で poison しない")
            .push(match_);
        Ok(())
    }

    async fn delete_match(&self, _match_id: MatchId) -> Result<(), CoreWriteError> {
        Ok(())
    }

    async fn append_fact(&self, _match_id: MatchId, fact: MatchFact) -> Result<(), CoreWriteError> {
        self.facts
            .lock()
            .expect("テスト内で poison しない")
            .push(fact);
        Ok(())
    }

    async fn update_fact(&self, _match_id: MatchId, fact: MatchFact) -> Result<(), CoreWriteError> {
        let mut facts = self.facts.lock().expect("テスト内で poison しない");
        if let Some(slot) = facts.iter_mut().find(|f| f.id == fact.id) {
            *slot = fact;
        }
        Ok(())
    }

    async fn delete_fact(&self, _match_id: MatchId, fact_id: FactId) -> Result<(), CoreWriteError> {
        self.facts
            .lock()
            .expect("テスト内で poison しない")
            .retain(|f| f.id != fact_id);
        Ok(())
    }
}

fn match_id() -> MatchId {
    MatchId(Uuid::from_u128(MATCH_ID))
}

// ── append ──

#[test]
fn 合格した_append_は発火する() {
    let repo = Arc::new(FakeRepo::new(vec![phase_start()]));
    let result = run(record_append_fact(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 60.0, Some(PlayerId(Uuid::from_u128(SCORER_ID)))),
    ));
    assert_eq!(result, Ok(()));
    assert_eq!(repo.fact_log().len(), 2);
}

#[test]
fn validation_違反の_append_は発火せず_validation_failed() {
    let repo = Arc::new(FakeRepo::new(vec![phase_start()]));
    let result = run(record_append_fact(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, -1.0, None),
    ));
    match result {
        Err(CoreWriteError::ValidationFailed { issues }) => {
            assert!(issues.contains(&DomainValidationIssue::Fact(
                FactValidationError::NegativeMatchClock
            )));
        }
        other => panic!("ValidationFailed を期待したが {other:?}"),
    }
    assert_eq!(repo.fact_log().len(), 1, "違反時は発火しない");
}

// ── roster skip ルール（シェルからコアへ移した判断）──

#[test]
fn roster_0_件なら参照整合を_skip_して発火する() {
    let unknown_player = PlayerId(Uuid::from_u128(99));
    let repo = Arc::new(FakeRepo::new(vec![phase_start()]));
    let result = run(record_append_fact(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 60.0, Some(unknown_player)),
    ));
    assert_eq!(
        result,
        Ok(()),
        "選手 0 件では dangling 参照を見ない（後方互換）"
    );
}

#[test]
fn roster_があると_dangling_参照は発火せず拒否() {
    let rostered = PlayerId(Uuid::from_u128(21));
    let unknown_player = PlayerId(Uuid::from_u128(99));
    let mut repo = FakeRepo::new(vec![phase_start()]);
    repo.roster_players = vec![PlayerTeamRef {
        player_id: rostered,
        team_id: TeamId(Uuid::from_u128(HOME_ID)),
    }];
    let repo = Arc::new(repo);

    let result = run(record_append_fact(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 60.0, Some(unknown_player)),
    ));
    match result {
        Err(CoreWriteError::ValidationFailed { issues }) => {
            assert!(issues.contains(&DomainValidationIssue::Fact(
                FactValidationError::UnknownPlayerReference {
                    player_id: unknown_player
                }
            )));
        }
        other => panic!("ValidationFailed を期待したが {other:?}"),
    }
    assert_eq!(repo.fact_log().len(), 1, "違反時は発火しない");
}

// ── update ──

#[test]
fn 合格した_update_は置換を発火する() {
    let scorer = Some(PlayerId(Uuid::from_u128(SCORER_ID)));
    let repo = Arc::new(FakeRepo::new(vec![
        phase_start(),
        goal(GOAL_ID, 60.0, scorer),
    ]));
    let result = run(record_update_fact(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 70.0, scorer),
    ));
    assert_eq!(result, Ok(()));
    let updated = repo
        .fact_log()
        .into_iter()
        .find(|f| f.id == FactId(Uuid::from_u128(GOAL_ID)))
        .expect("置換後も存在する");
    assert_eq!(
        updated.anchor(),
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 70.0
        })
    );
}

#[test]
fn validation_違反の_update_は発火しない() {
    let repo = Arc::new(FakeRepo::new(vec![
        phase_start(),
        goal(GOAL_ID, 60.0, None),
    ]));
    let result = run(record_update_fact(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, -1.0, None),
    ));
    assert!(matches!(
        result,
        Err(CoreWriteError::ValidationFailed { .. })
    ));
    let unchanged = repo
        .fact_log()
        .into_iter()
        .find(|f| f.id == FactId(Uuid::from_u128(GOAL_ID)))
        .expect("存在する");
    assert_eq!(
        unchanged.anchor(),
        FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 60.0
        })
    );
}

// ── delete ──

#[test]
fn 合格した_delete_は発火する() {
    let repo = Arc::new(FakeRepo::new(vec![
        phase_start(),
        goal(GOAL_ID, 60.0, None),
    ]));
    let result = run(record_delete_fact(
        repo.clone(),
        match_id(),
        FactId(Uuid::from_u128(GOAL_ID)),
    ));
    assert_eq!(result, Ok(()));
    assert_eq!(repo.fact_log().len(), 1);
}

#[test]
fn play_を内包する_phase_start_の削除は発火せず拒否() {
    let repo = Arc::new(FakeRepo::new(vec![
        phase_start(),
        goal(GOAL_ID, 60.0, None),
    ]));
    let result = run(record_delete_fact(
        repo.clone(),
        match_id(),
        FactId(Uuid::from_u128(PHASE_START_ID)),
    ));
    assert!(matches!(
        result,
        Err(CoreWriteError::ValidationFailed { .. })
    ));
    assert_eq!(repo.fact_log().len(), 2, "違反時は発火しない");
}

// ── phase 自動補完込み記録（実装順序 3）──

fn stamp(id: u128) -> NewFactStamp {
    NewFactStamp {
        id: FactId(Uuid::from_u128(id)),
        recorded_at: chrono::DateTime::from_timestamp(id as i64, 0).expect("固定秒は有効"),
    }
}

#[test]
fn 必要数はコアが数える() {
    let repo = Arc::new(FakeRepo::new(vec![]));
    let count = run(count_phase_completion_facts(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 1900.0, Some(PlayerId(Uuid::from_u128(SCORER_ID)))),
    ));
    assert_eq!(count, Ok(2), "後半の記録は前半 + 後半の 2 phase を要する");
}

#[test]
fn 補完込み記録は_phase_を連鎖発火してから本_fact_を発火する() {
    let repo = Arc::new(FakeRepo::new(vec![]));
    let result = run(record_fact_with_phase_completion(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 1900.0, Some(PlayerId(Uuid::from_u128(SCORER_ID)))),
        vec![stamp(301), stamp(302)],
    ));
    assert_eq!(result, Ok(()));

    let log = repo.fact_log();
    assert_eq!(log.len(), 3, "phase 2 件 + goal 1 件");
    // スタンプは消費順（前半 → 後半）に使われる。
    assert_eq!(log[0].id, FactId(Uuid::from_u128(301)));
    assert_eq!(log[1].id, FactId(Uuid::from_u128(302)));
    assert_eq!(log[2].id, FactId(Uuid::from_u128(GOAL_ID)));
}

#[test]
fn 既存_phase_があれば欠けだけ補完する() {
    let repo = Arc::new(FakeRepo::new(vec![phase_start()]));
    let result = run(record_fact_with_phase_completion(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 60.0, Some(PlayerId(Uuid::from_u128(SCORER_ID)))),
        vec![],
    ));
    assert_eq!(result, Ok(()), "phase 1 は既存なので補完 0 件で成立する");
    assert_eq!(repo.fact_log().len(), 2);
}

#[test]
fn スタンプ不足は発火せず_insufficient_new_ids() {
    let repo = Arc::new(FakeRepo::new(vec![]));
    let result = run(record_fact_with_phase_completion(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 1900.0, Some(PlayerId(Uuid::from_u128(SCORER_ID)))),
        vec![stamp(301)],
    ));
    assert_eq!(
        result,
        Err(CoreWriteError::InsufficientNewIds {
            required: 2,
            provided: 1
        })
    );
    assert!(repo.fact_log().is_empty(), "不足時は何も発火しない");
}

#[test]
fn 本_fact_の違反は補完_phase_発火後でも拒否される() {
    let repo = Arc::new(FakeRepo::new(vec![]));
    // Goal は player 必須 → 補完 phase は発火するが本 fact は ValidationFailed。
    let result = run(record_fact_with_phase_completion(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 60.0, None),
        vec![stamp(301)],
    ));
    assert!(matches!(
        result,
        Err(CoreWriteError::ValidationFailed { .. })
    ));
    // 現行挙動パリティ: 連鎖は非 atomic — 発火済み補完 phase は残る。
    assert_eq!(repo.fact_log().len(), 1);
}

// ── video 移行 commit（実装順序 4）──

fn poc_video_source() -> VideoSource {
    VideoSource {
        provider: VideoProvider::Youtube,
        external_id: "poc".to_string(),
    }
}

#[test]
fn video_移行_commit_は_config_先行_save_後に_facts_を計画順に更新する() {
    let repo = Arc::new(FakeRepo::new(vec![
        phase_start(),
        goal(GOAL_ID, 60.0, Some(PlayerId(Uuid::from_u128(SCORER_ID)))),
    ]));
    let result = run(commit_video_migration(
        repo.clone(),
        match_id(),
        poc_video_source(),
        vec![VideoSyncInput {
            fact_id: FactId(Uuid::from_u128(PHASE_START_ID)),
            video_start_seconds: 10.0,
            video_end_seconds: 1810.0,
        }],
        vec![],
    ));
    assert_eq!(result, Ok(()));

    // config は先行 save で .video 化されている。
    let saved = repo
        .saved_matches
        .lock()
        .expect("テスト内で poison しない")
        .clone();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0].configuration,
        MatchConfiguration::Video(poc_video_source())
    );

    // facts: phase は both、goal は videoClock(10 + 60 = 70)へ変換済み。
    let log = repo.fact_log();
    let phase = log
        .iter()
        .find(|f| f.id == FactId(Uuid::from_u128(PHASE_START_ID)));
    assert!(matches!(
        phase.map(|f| f.anchor()),
        Some(FactAnchor::Both { .. })
    ));
    let goal_fact = log
        .iter()
        .find(|f| f.id == FactId(Uuid::from_u128(GOAL_ID)))
        .expect("goal は存在する");
    assert_eq!(
        goal_fact.anchor(),
        FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 70.0
        })
    );
}

#[test]
fn video_移行_commit_は_sync_欠落なら何も発火しない() {
    let repo = Arc::new(FakeRepo::new(vec![
        phase_start(),
        goal(GOAL_ID, 60.0, Some(PlayerId(Uuid::from_u128(SCORER_ID)))),
    ]));
    let result = run(commit_video_migration(
        repo.clone(),
        match_id(),
        poc_video_source(),
        vec![],
        vec![],
    ));
    assert!(matches!(
        result,
        Err(CoreWriteError::MigrationPlanInfeasible { .. })
    ));
    assert!(
        repo.saved_matches
            .lock()
            .expect("テスト内で poison しない")
            .is_empty(),
        "計画不成立なら config も書かない"
    );
    assert!(matches!(
        repo.fact_log()[1].anchor(),
        FactAnchor::MatchClock(_)
    ));
}

// ── repository 失敗の伝播 ──

#[test]
fn repository_の失敗はそのまま伝播する() {
    let mut repo = FakeRepo::new(vec![phase_start()]);
    repo.fail_load_match = true;
    let repo = Arc::new(repo);

    let result = run(record_append_fact(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 60.0, None),
    ));
    assert_eq!(
        result,
        Err(CoreWriteError::Repository {
            message: "load_match 失敗".to_string()
        })
    );
    assert_eq!(repo.fact_log().len(), 1, "読み取り失敗時は発火しない");
}

// ── entity CRUD 入口（実装順序 5）──

/// 素朴 CRUD の team-scope fake。使用中判定がコア側にあることを削除の発火有無で観測する。
#[derive(Debug, Default)]
struct FakeTeamRepo {
    match_refs: u32,
    fact_refs: u32,
    saved_teams: Mutex<Vec<Team>>,
    saved_players: Mutex<Vec<Player>>,
    deleted_teams: Mutex<Vec<TeamId>>,
    deleted_players: Mutex<Vec<PlayerId>>,
}

#[async_trait::async_trait]
impl TeamWriteRepository for FakeTeamRepo {
    async fn count_matches_referencing_team(
        &self,
        _team_id: TeamId,
    ) -> Result<u32, CoreWriteError> {
        Ok(self.match_refs)
    }

    async fn count_facts_referencing_player(
        &self,
        _player_id: PlayerId,
    ) -> Result<u32, CoreWriteError> {
        Ok(self.fact_refs)
    }

    async fn save_team(&self, team: Team) -> Result<(), CoreWriteError> {
        self.saved_teams
            .lock()
            .expect("テスト内で poison しない")
            .push(team);
        Ok(())
    }

    async fn delete_team(&self, team_id: TeamId) -> Result<(), CoreWriteError> {
        self.deleted_teams
            .lock()
            .expect("テスト内で poison しない")
            .push(team_id);
        Ok(())
    }

    async fn save_player(&self, player: Player) -> Result<(), CoreWriteError> {
        self.saved_players
            .lock()
            .expect("テスト内で poison しない")
            .push(player);
        Ok(())
    }

    async fn delete_player(&self, player_id: PlayerId) -> Result<(), CoreWriteError> {
        self.deleted_players
            .lock()
            .expect("テスト内で poison しない")
            .push(player_id);
        Ok(())
    }
}

#[test]
fn 使用中チームの削除は発火せず_team_in_use() {
    let repo = Arc::new(FakeTeamRepo {
        match_refs: 3,
        ..FakeTeamRepo::default()
    });
    let result = run(record_delete_team(
        repo.clone(),
        TeamId(Uuid::from_u128(HOME_ID)),
    ));
    assert_eq!(result, Err(CoreWriteError::TeamInUse { match_count: 3 }));
    assert!(
        repo.deleted_teams
            .lock()
            .expect("テスト内で poison しない")
            .is_empty()
    );
}

#[test]
fn 未使用チームの削除は発火する() {
    let repo = Arc::new(FakeTeamRepo::default());
    let result = run(record_delete_team(
        repo.clone(),
        TeamId(Uuid::from_u128(HOME_ID)),
    ));
    assert_eq!(result, Ok(()));
    assert_eq!(
        repo.deleted_teams
            .lock()
            .expect("テスト内で poison しない")
            .as_slice(),
        &[TeamId(Uuid::from_u128(HOME_ID))]
    );
}

#[test]
fn 使用中選手の削除は発火せず_player_in_use() {
    let repo = Arc::new(FakeTeamRepo {
        fact_refs: 2,
        ..FakeTeamRepo::default()
    });
    let result = run(record_delete_player(
        repo.clone(),
        PlayerId(Uuid::from_u128(SCORER_ID)),
    ));
    assert_eq!(result, Err(CoreWriteError::PlayerInUse { fact_count: 2 }));
    assert!(
        repo.deleted_players
            .lock()
            .expect("テスト内で poison しない")
            .is_empty()
    );
}

#[test]
fn 未使用選手の削除と_save_の_passthrough_は発火する() {
    let repo = Arc::new(FakeTeamRepo::default());
    assert_eq!(
        run(record_delete_player(
            repo.clone(),
            PlayerId(Uuid::from_u128(SCORER_ID))
        )),
        Ok(())
    );
    let team = Team {
        id: TeamId(Uuid::from_u128(HOME_ID)),
        name: "Tigers".to_string(),
    };
    assert_eq!(run(record_save_team(repo.clone(), team)), Ok(()));
    let player = Player {
        id: PlayerId(Uuid::from_u128(SCORER_ID)),
        team_id: TeamId(Uuid::from_u128(HOME_ID)),
        name: "Alice".to_string(),
        jersey_number: Some(7),
        photo: None,
    };
    assert_eq!(run(record_save_player(repo.clone(), player)), Ok(()));
    assert_eq!(
        repo.saved_teams
            .lock()
            .expect("テスト内で poison しない")
            .len(),
        1
    );
    assert_eq!(
        repo.saved_players
            .lock()
            .expect("テスト内で poison しない")
            .len(),
        1
    );
}

// ── サンプル試合 import commit（handball-project#67 / atomic 化 #83）──
//
// 計画層（ID 解決・集計・並び）は handball-toolkit の `sample_import_tests` が固定し、
// 検証 + バッチ組立（`import_commit_batch`）も同 crate のテストが固定する。
// ここで見るのは発火層の責務: 検証を通ったら **1 バッチ**を `commit_import` へ渡すこと
// （= 1 context.save() で atomic）と、拒否経路（ID 不足 / decode 失敗 / 検証失敗）で
// commit_import を 1 度も発火しないこと。

/// import commit の atomic 発火を記録する fake。`commit_import` は受け取ったバッチを溜める。
#[derive(Debug, Default)]
struct FakeImportRepo {
    committed: Mutex<Vec<ImportWriteBatch>>,
}

#[async_trait::async_trait]
impl ImportWriteRepository for FakeImportRepo {
    async fn commit_import(&self, batch: ImportWriteBatch) -> Result<(), CoreWriteError> {
        self.committed
            .lock()
            .expect("テスト内で poison しない")
            .push(batch);
        Ok(())
    }
}

/// import 由来の match（ID 供給を連番にしたときの結果と一致させる）。
fn imported_match(id: u128, home: u128, away: u128) -> Match {
    Match {
        id: MatchId(Uuid::from_u128(id)),
        title: Some("テスト試合".to_string()),
        date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
        home_team_id: TeamId(Uuid::from_u128(home)),
        away_team_id: TeamId(Uuid::from_u128(away)),
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    }
}

fn import_anchor(start: f64, end: Option<f64>) -> SampleFactAnchorDtoV2 {
    SampleFactAnchorDtoV2 {
        kind: "matchClock".to_string(),
        match_clock: Some(SampleMatchClockDtoV2 {
            elapsed_seconds: start,
        }),
        video_clock: None,
        end_match_elapsed_seconds: end,
        end_video_elapsed_seconds: None,
    }
}

fn import_dto(player_key: Option<&str>) -> SampleMatchDtoV2 {
    SampleMatchDtoV2 {
        schema_version: SCHEMA_VERSION_CURRENT,
        r#match: SampleMatchHeaderV2 {
            display_name: Some("テスト試合".to_string()),
            date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
            configuration: SampleMatchConfigurationDtoV2 {
                kind: "timer".to_string(),
                timer: Some(SampleTimerConfigurationDtoV2 {
                    phase_duration_seconds: 1800.0,
                }),
                video: None,
                video_highlight: None,
            },
        },
        teams: SampleTeamsDtoV2 {
            home: SampleTeamDtoV2 {
                key: "home".to_string(),
                name: "ホーム".to_string(),
                players: vec![
                    SamplePlayerDtoV2 {
                        key: "h1".to_string(),
                        name: "ホーム1".to_string(),
                        jersey_number: Some(1),
                    },
                    SamplePlayerDtoV2 {
                        key: "h2".to_string(),
                        name: "ホーム2".to_string(),
                        jersey_number: Some(2),
                    },
                ],
            },
            away: SampleTeamDtoV2 {
                key: "away".to_string(),
                name: "アウェイ".to_string(),
                players: vec![SamplePlayerDtoV2 {
                    key: "a1".to_string(),
                    name: "アウェイ1".to_string(),
                    jersey_number: Some(1),
                }],
            },
        },
        facts: vec![
            SampleFactDtoV2 {
                fact_id: None,
                recorded_at: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
                payload: SampleFactPayloadDtoV2 {
                    kind: "control".to_string(),
                    play: None,
                    control: Some(SampleControlFactDtoV2 {
                        kind: "phaseStart".to_string(),
                        phase_start: Some(SamplePhaseStartPayloadDtoV2 {
                            kind: "regular".to_string(),
                        }),
                        stoppage: None,
                        anchor: import_anchor(0.0, Some(1800.0)),
                    }),
                },
            },
            SampleFactDtoV2 {
                fact_id: None,
                recorded_at: chrono::DateTime::from_timestamp(1, 0).expect("固定秒は有効"),
                payload: SampleFactPayloadDtoV2 {
                    kind: "play".to_string(),
                    play: Some(SamplePlayFactDtoV2 {
                        kind: "goal".to_string(),
                        team_key: Some("home".to_string()),
                        player_key: player_key.map(str::to_string),
                        related_player_key: None,
                        anchor: import_anchor(600.0, None),
                        title: None,
                        note: None,
                    }),
                    control: None,
                },
            },
        ],
    }
}

fn all_new_decisions() -> ImportDecisions {
    ImportDecisions {
        home_team: TeamTarget::CreateNew,
        away_team: TeamTarget::CreateNew,
        players: HashMap::new(),
    }
}

/// 連番 ID（100 起点）。既存 fixture の ID 空間と衝突させない。
fn import_ids(count: usize) -> Vec<Uuid> {
    (0..count)
        .map(|i| Uuid::from_u128(100 + i as u128))
        .collect()
}

#[test]
fn import_commit_は_1_バッチで_team_player_match_fact_を発火する() {
    // 連番 ID: home=100 away=101 / players=102,103,104 / match=105 / facts=106,107
    let match_repo = Arc::new(FakeRepo {
        match_: imported_match(105, 100, 101),
        ..FakeRepo::new(Vec::new())
    });
    let import_repo = Arc::new(FakeImportRepo::default());

    let outcome = run(commit_sample_match_import(
        match_repo.clone(),
        import_repo.clone(),
        import_dto(Some("h1")),
        all_new_decisions(),
        import_ids(8),
    ))
    .expect("全新規の import は成功する");

    assert_eq!(outcome.teams_created, 2);
    assert_eq!(outcome.teams_reused, 0);
    assert_eq!(outcome.players_created, 3);
    assert_eq!(outcome.players_reused, 0);
    assert_eq!(outcome.fact_count, 2);
    assert_eq!(outcome.match_id, MatchId(Uuid::from_u128(105)));

    // 発火は 1 バッチだけ（= 1 context.save() で atomic）。
    let committed = import_repo
        .committed
        .lock()
        .expect("テスト内で poison しない");
    assert_eq!(committed.len(), 1, "commit_import は 1 回だけ発火する");
    let batch = &committed[0];
    assert_eq!(
        batch
            .teams
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        vec!["ホーム", "アウェイ"]
    );
    assert_eq!(
        batch
            .players
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["ホーム1", "ホーム2", "アウェイ1"]
    );
    assert_eq!(batch.match_.id, MatchId(Uuid::from_u128(105)));
    // facts は永続化順（累積秒 0s の phaseStart → 600s の goal）。
    assert_eq!(batch.facts.len(), 2);
    assert!(matches!(
        batch.facts[0].payload,
        MatchFactPayload::Control(_)
    ));
    assert!(matches!(batch.facts[1].payload, MatchFactPayload::Play(_)));

    // match_repo へは検証入力の read のみ。逐次 save_match / append_fact は通らない。
    assert!(
        match_repo
            .saved_matches
            .lock()
            .expect("テスト内で poison しない")
            .is_empty()
    );
    assert!(match_repo.fact_log().is_empty());
}

#[test]
fn 事前生成_id_が不足した_import_は発火せず_insufficient_new_ids() {
    let match_repo = Arc::new(FakeRepo {
        match_: imported_match(105, 100, 101),
        ..FakeRepo::new(Vec::new())
    });
    let import_repo = Arc::new(FakeImportRepo::default());

    let result = run(commit_sample_match_import(
        match_repo.clone(),
        import_repo.clone(),
        import_dto(Some("h1")),
        all_new_decisions(),
        import_ids(7),
    ));

    assert_eq!(
        result,
        Err(CoreWriteError::InsufficientNewIds {
            required: 8,
            provided: 7
        })
    );
    assert!(
        import_repo
            .committed
            .lock()
            .expect("テスト内で poison しない")
            .is_empty()
    );
}

#[test]
fn 未知の_player_key_を含む_import_は発火せず_import_decode_failed() {
    let match_repo = Arc::new(FakeRepo {
        match_: imported_match(105, 100, 101),
        ..FakeRepo::new(Vec::new())
    });
    let import_repo = Arc::new(FakeImportRepo::default());

    let result = run(commit_sample_match_import(
        match_repo.clone(),
        import_repo.clone(),
        import_dto(Some("ghost")),
        all_new_decisions(),
        import_ids(8),
    ));

    assert!(matches!(
        result,
        Err(CoreWriteError::ImportDecodeFailed { .. })
    ));
    // 計画が失敗した時点で 1 件も発火しない（decode は組立より前）。
    assert!(
        import_repo
            .committed
            .lock()
            .expect("テスト内で poison しない")
            .is_empty()
    );
}

#[test]
fn 検証に落ちる_import_は_commit_import_を発火しない() {
    let match_repo = Arc::new(FakeRepo {
        match_: imported_match(105, 100, 101),
        ..FakeRepo::new(Vec::new())
    });
    let import_repo = Arc::new(FakeImportRepo::default());

    // goal に player が無い → MissingPlayerForPlayKind で検証に落ちる（計画は成立する）。
    let result = run(commit_sample_match_import(
        match_repo.clone(),
        import_repo.clone(),
        import_dto(None),
        all_new_decisions(),
        import_ids(8),
    ));

    assert!(matches!(
        result,
        Err(CoreWriteError::ValidationFailed { .. })
    ));
    assert!(
        import_repo
            .committed
            .lock()
            .expect("テスト内で poison しない")
            .is_empty(),
        "検証に落ちたら 1 件も保存しない（atomic の片側の保証）"
    );
}
