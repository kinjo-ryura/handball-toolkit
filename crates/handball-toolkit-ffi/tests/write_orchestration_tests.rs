//! write 入口（発火層 `ffi_write` — ADR 0005 実装順序 1）の挙動固定。
//!
//! fake repository で「読む → 検証 → 発火」の orchestration を FFI を越える前に固定する:
//! 合格時のみ発火・違反は `ValidationFailed` で不発火・repository 失敗の伝播・
//! roster 0 件 skip ルール（コアへ移した判断）。async 入口は pollster で回す
//! （fake は即時完了 future のためランタイム不要）。

use std::future::Future;
use std::sync::{Arc, Mutex};

use handball_toolkit::clock::{FactAnchor, MatchClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind};
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
};
use handball_toolkit::ids::{FactId, MatchId, PlayerId, TeamId};
use handball_toolkit::validation::{DomainValidationIssue, FactValidationError};
use handball_toolkit::write::PlayerTeamRef;
use handball_toolkit_ffi::ffi_write::{
    CoreWriteError, MatchWriteRepository, record_append_fact, record_delete_fact,
    record_update_fact,
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
        id: Uuid::from_u128(MATCH_ID),
        title: Some("write orchestration".to_string()),
        date: chrono::DateTime::from_timestamp(0, 0).expect("epoch は有効"),
        home_team_id: Uuid::from_u128(HOME_ID),
        away_team_id: Uuid::from_u128(AWAY_ID),
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    }
}

fn fact(id: u128, payload: MatchFactPayload) -> MatchFact {
    MatchFact {
        id: Uuid::from_u128(id),
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
            team_id: Some(Uuid::from_u128(HOME_ID)),
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
}

impl FakeRepo {
    fn new(facts: Vec<MatchFact>) -> Self {
        FakeRepo {
            match_: timer_match(),
            facts: Mutex::new(facts),
            roster_players: Vec::new(),
            fail_load_match: false,
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

    async fn save_match(&self, _match_: Match) -> Result<(), CoreWriteError> {
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
    Uuid::from_u128(MATCH_ID)
}

// ── append ──

#[test]
fn 合格した_append_は発火する() {
    let repo = Arc::new(FakeRepo::new(vec![phase_start()]));
    let result = run(record_append_fact(
        repo.clone(),
        match_id(),
        goal(GOAL_ID, 60.0, Some(Uuid::from_u128(SCORER_ID))),
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
    let unknown_player = Uuid::from_u128(99);
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
    let rostered = Uuid::from_u128(21);
    let unknown_player = Uuid::from_u128(99);
    let mut repo = FakeRepo::new(vec![phase_start()]);
    repo.roster_players = vec![PlayerTeamRef {
        player_id: rostered,
        team_id: Uuid::from_u128(HOME_ID),
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
    let scorer = Some(Uuid::from_u128(SCORER_ID));
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
        .find(|f| f.id == Uuid::from_u128(GOAL_ID))
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
        .find(|f| f.id == Uuid::from_u128(GOAL_ID))
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
        Uuid::from_u128(GOAL_ID),
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
        Uuid::from_u128(PHASE_START_ID),
    ));
    assert!(matches!(
        result,
        Err(CoreWriteError::ValidationFailed { .. })
    ));
    assert_eq!(repo.fact_log().len(), 2, "違反時は発火しない");
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
