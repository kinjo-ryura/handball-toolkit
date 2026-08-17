//! 移植元: `HandballRecorderTests/MatchMergerV2Tests.swift`（6 件）と
//! `MatchImporterV2CommitTests.swift` の commit 計画部分（handball-project#67）。
//!
//! merge 候補算出（exact / partial 照合・候補ソート・default decisions・名前正規化）と
//! commit 計画（ID 解決・保存順序・集計）の挙動を固定する。永続化の発火は
//! `ffi_write::commit_sample_match_import` 側のテスト（fake repo）で見る。

use std::collections::HashMap;

use chrono::{DateTime, TimeZone, Utc};
use handball_toolkit::entities::{Player, Team};
use handball_toolkit::facts::MatchFactPayload;
use handball_toolkit::ids::{PlayerId, TeamId};
use handball_toolkit::sample_dto::{
    SCHEMA_VERSION_CURRENT, SampleControlFactDtoV2, SampleFactAnchorDtoV2, SampleFactDtoV2,
    SampleFactPayloadDtoV2, SampleMatchClockDtoV2, SampleMatchConfigurationDtoV2, SampleMatchDtoV2,
    SampleMatchHeaderV2, SamplePhaseStartPayloadDtoV2, SamplePlayFactDtoV2, SamplePlayerDtoV2,
    SampleTeamDtoV2, SampleTeamsDtoV2, SampleTimerConfigurationDtoV2,
};
use handball_toolkit::sample_import::{
    ExistingSnapshot, ImportDecisions, PlayerTarget, TeamOption, TeamTarget, default_decisions,
    find_team_options, import_commit_batch, import_commit_plan, normalize_name,
    required_import_id_count,
};
use uuid::Uuid;

// ── helpers ──

fn epoch(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0).unwrap()
}

fn parsed_team(name: &str, players: &[(&str, Option<i64>)]) -> SampleTeamDtoV2 {
    SampleTeamDtoV2 {
        key: "home".to_owned(),
        name: name.to_owned(),
        players: players
            .iter()
            .enumerate()
            .map(|(i, (player_name, jersey))| SamplePlayerDtoV2 {
                key: format!("p{i}"),
                name: (*player_name).to_owned(),
                jersey_number: *jersey,
            })
            .collect(),
    }
}

fn team(name: &str) -> Team {
    Team {
        id: TeamId(Uuid::new_v4()),
        name: name.to_owned(),
    }
}

fn player(team_id: TeamId, name: &str, jersey: Option<i64>) -> Player {
    Player {
        id: PlayerId(Uuid::new_v4()),
        team_id,
        name: name.to_owned(),
        jersey_number: jersey,
        photo: None,
    }
}

/// 決定的な ID 供給（連番 UUID）。消費順の検証に使う。
fn sequential_ids() -> impl FnMut() -> Uuid {
    let mut counter: u128 = 0;
    move || {
        counter += 1;
        Uuid::from_u128(counter)
    }
}

// ── findTeamOptions ──

#[test]
fn appends_new_team_option_even_with_no_existing() {
    let parsed = parsed_team("Sample", &[("Alice", Some(7)), ("Bob", Some(10))]);
    let snapshot = ExistingSnapshot::default();

    let options = find_team_options(&parsed, &snapshot);

    assert_eq!(options.len(), 1);
    assert!(options[0].existing.is_none());
    assert_eq!(options[0].parsed_only, vec!["p0", "p1"]);
}

#[test]
fn ranks_by_exact_match_count_descending() {
    let parsed = parsed_team("Sample", &[("Alice", Some(7)), ("Bob", Some(10))]);
    let team_a = team("Team A");
    let team_b = team("Team B");
    let snapshot = ExistingSnapshot {
        teams: vec![team_a.clone(), team_b.clone()],
        players: vec![
            player(team_a.id, "Alice", Some(7)),
            player(team_a.id, "Bob", Some(10)),
            player(team_b.id, "Alice", Some(7)),
        ],
    };

    let options = find_team_options(&parsed, &snapshot);

    // team_a は 2 完全一致、team_b は 1 完全一致。
    assert_eq!(options[0].existing.as_ref().map(|t| t.id), Some(team_a.id));
    assert_eq!(options[0].exact_matches.len(), 2);
    assert_eq!(options[1].existing.as_ref().map(|t| t.id), Some(team_b.id));
    assert_eq!(options[1].exact_matches.len(), 1);
    // 末尾に新規作成。
    assert!(options.last().unwrap().existing.is_none());
}

#[test]
fn detects_partial_match_by_jersey_only() {
    let parsed = parsed_team("Sample", &[("Alice", Some(7))]);
    let team_a = team("Team A");
    let snapshot = ExistingSnapshot {
        teams: vec![team_a.clone()],
        players: vec![player(team_a.id, "Alicia", Some(7))],
    };

    let options = find_team_options(&parsed, &snapshot);

    let option = options
        .iter()
        .find(|o| o.existing.as_ref().map(|t| t.id) == Some(team_a.id))
        .unwrap();
    assert!(option.exact_matches.is_empty());
    assert_eq!(option.partial_matches.len(), 1);
    assert_eq!(option.partial_matches[0].candidates[0].name, "Alicia");
}

#[test]
fn reports_existing_only_players() {
    let parsed = parsed_team("Sample", &[("Alice", Some(7))]);
    let team_a = team("Team A");
    let snapshot = ExistingSnapshot {
        teams: vec![team_a.clone()],
        players: vec![
            player(team_a.id, "Alice", Some(7)),
            player(team_a.id, "Carol", Some(99)),
        ],
    };

    let options = find_team_options(&parsed, &snapshot);

    let option = options
        .iter()
        .find(|o| o.existing.as_ref().map(|t| t.id) == Some(team_a.id))
        .unwrap();
    let names: Vec<&str> = option
        .existing_only
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, vec!["Carol"]);
}

// ── defaultDecisions ──

#[test]
fn default_decisions_match_exact_reuses_partial_first_candidates_new_team_creates_all() {
    let parsed = parsed_team(
        "Sample",
        &[
            ("Alice", Some(7)), // exact
            ("Bob", Some(99)),  // jersey-only partial
            ("Dora", None),     // parsedOnly
        ],
    );
    let team_a = team("Team A");
    let exact_player = player(team_a.id, "Alice", Some(7));
    let partial_candidate = player(team_a.id, "Brett", Some(99));
    let snapshot = ExistingSnapshot {
        teams: vec![team_a.clone()],
        players: vec![exact_player.clone(), partial_candidate.clone()],
    };

    let options = find_team_options(&parsed, &snapshot);
    let home_option = options[0].clone();
    let away_option = TeamOption::new_team(vec!["away_p0".to_owned()]);

    let decisions = default_decisions(&home_option, &away_option);

    assert_eq!(
        decisions.home_team,
        TeamTarget::Existing { team_id: team_a.id }
    );
    assert_eq!(decisions.away_team, TeamTarget::CreateNew);
    assert_eq!(
        decisions.players["p0"],
        PlayerTarget::Existing {
            player_id: exact_player.id
        }
    );
    assert_eq!(
        decisions.players["p1"],
        PlayerTarget::Existing {
            player_id: partial_candidate.id
        }
    );
    assert_eq!(decisions.players["p2"], PlayerTarget::CreateNew);
}

// ── normalize ──

#[test]
fn normalize_collapses_and_strips_whitespace() {
    assert_eq!(normalize_name("  山田　太郎 "), "山田 太郎");
    assert_eq!(normalize_name("田中 \t 一郎"), "田中 一郎");
}

// ── commit 計画 ──

fn anchor_dto(match_seconds: Option<f64>, end_match: Option<f64>) -> SampleFactAnchorDtoV2 {
    SampleFactAnchorDtoV2 {
        kind: "matchClock".to_owned(),
        match_clock: match_seconds.map(|s| SampleMatchClockDtoV2 { elapsed_seconds: s }),
        video_clock: None,
        end_match_elapsed_seconds: end_match,
        end_video_elapsed_seconds: None,
    }
}

fn play_fact_dto(player_key: Option<&str>) -> SampleFactDtoV2 {
    SampleFactDtoV2 {
        fact_id: None,
        recorded_at: epoch(1),
        payload: SampleFactPayloadDtoV2 {
            kind: "play".to_owned(),
            play: Some(SamplePlayFactDtoV2 {
                kind: "goal".to_owned(),
                team_key: Some("home".to_owned()),
                player_key: player_key.map(str::to_owned),
                related_player_key: None,
                anchor: anchor_dto(Some(600.0), None),
                title: None,
                note: None,
            }),
            control: None,
            possession: None,
        },
    }
}

fn phase_start_fact_dto() -> SampleFactDtoV2 {
    SampleFactDtoV2 {
        fact_id: None,
        recorded_at: epoch(0),
        payload: SampleFactPayloadDtoV2 {
            kind: "control".to_owned(),
            play: None,
            control: Some(SampleControlFactDtoV2 {
                kind: "phaseStart".to_owned(),
                phase_start: Some(SamplePhaseStartPayloadDtoV2 {
                    kind: "regular".to_owned(),
                }),
                stoppage: None,
                anchor: anchor_dto(Some(0.0), Some(1800.0)),
            }),
            possession: None,
        },
    }
}

fn import_dto(facts: Vec<SampleFactDtoV2>) -> SampleMatchDtoV2 {
    SampleMatchDtoV2 {
        schema_version: SCHEMA_VERSION_CURRENT,
        r#match: SampleMatchHeaderV2 {
            display_name: Some("テスト試合".to_owned()),
            date: epoch(0),
            configuration: SampleMatchConfigurationDtoV2 {
                kind: "timer".to_owned(),
                timer: Some(SampleTimerConfigurationDtoV2 {
                    phase_duration_seconds: 1800.0,
                }),
                video: None,
                video_highlight: None,
            },
        },
        teams: SampleTeamsDtoV2 {
            home: SampleTeamDtoV2 {
                key: "home".to_owned(),
                name: "ホーム".to_owned(),
                players: vec![
                    SamplePlayerDtoV2 {
                        key: "h1".to_owned(),
                        name: "ホーム1".to_owned(),
                        jersey_number: Some(1),
                    },
                    SamplePlayerDtoV2 {
                        key: "h2".to_owned(),
                        name: "ホーム2".to_owned(),
                        jersey_number: Some(2),
                    },
                ],
            },
            away: SampleTeamDtoV2 {
                key: "away".to_owned(),
                name: "アウェイ".to_owned(),
                players: vec![SamplePlayerDtoV2 {
                    key: "a1".to_owned(),
                    name: "アウェイ1".to_owned(),
                    jersey_number: Some(1),
                }],
            },
        },
        facts,
    }
}

fn default_import_dto() -> SampleMatchDtoV2 {
    import_dto(vec![phase_start_fact_dto(), play_fact_dto(Some("h1"))])
}

/// DTO の facts が記録順（時刻順とは限らない）で来ても、plan は永続化順
/// （累積秒 → recordedAt → id）へ整列して返す。
///
/// 配信中の `.video` サンプルは phase 開始が配列の後方にあり、DTO 順のまま逐次 append すると
/// 最初の play を積んだ時点で whole-log 検証の R3 / R5 に抵触して必ず失敗していた
/// （handball-project#72）。整列はその回帰止め。
#[test]
fn commit_plan_sorts_facts_into_persistence_order() {
    // DTO 順は [play(600s), phaseStart(0s)] = 時刻の逆順。
    let dto = import_dto(vec![play_fact_dto(Some("h1")), phase_start_fact_dto()]);
    let decisions = ImportDecisions {
        home_team: TeamTarget::CreateNew,
        away_team: TeamTarget::CreateNew,
        players: HashMap::new(),
    };

    let plan = import_commit_plan(&dto, &decisions, sequential_ids()).unwrap();

    assert_eq!(plan.facts.len(), 2);
    assert!(
        matches!(plan.facts[0].payload, MatchFactPayload::Control(_)),
        "累積秒 0s の phaseStart が先頭に来るべき（DTO では 2 番目）"
    );
    assert!(
        matches!(plan.facts[1].payload, MatchFactPayload::Play(_)),
        "累積秒 600s の play が後ろに来るべき（DTO では 1 番目）"
    );
}

/// home=existing / away=createNew、選手 h1=existing / 残り default createNew の混在解決で
/// created / reused を正しく数え、既存に統合したチーム・選手は save 対象に積まない。
#[test]
fn commit_plan_counts_created_reused_and_saves_only_new_entities() {
    let existing_team_id = TeamId(Uuid::from_u128(0xAA));
    let existing_player_id = PlayerId(Uuid::from_u128(0xBB));
    let dto = default_import_dto();
    let decisions = ImportDecisions {
        home_team: TeamTarget::Existing {
            team_id: existing_team_id,
        },
        away_team: TeamTarget::CreateNew,
        // h2 / a1 は decisions に無い = default createNew。
        players: HashMap::from([(
            "h1".to_owned(),
            PlayerTarget::Existing {
                player_id: existing_player_id,
            },
        )]),
    };

    let plan = import_commit_plan(&dto, &decisions, sequential_ids()).unwrap();

    assert_eq!(plan.outcome.teams_reused, 1);
    assert_eq!(plan.outcome.teams_created, 1);
    assert_eq!(plan.outcome.players_reused, 1);
    assert_eq!(plan.outcome.players_created, 2);
    assert_eq!(plan.outcome.fact_count, 2);

    // createNew の team のみ save 対象（existing は save しない）。
    assert_eq!(plan.teams_to_save.len(), 1);
    assert_eq!(plan.teams_to_save[0].name, "アウェイ");
    assert_eq!(plan.outcome.home_team_id, existing_team_id);
    assert_eq!(plan.outcome.away_team_id, plan.teams_to_save[0].id);

    // createNew の選手のみ save 対象。home 所属 → away 所属の順。
    let names: Vec<&str> = plan
        .players_to_save
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, vec!["ホーム2", "アウェイ1"]);
    assert_eq!(plan.players_to_save[0].team_id, existing_team_id);
    assert_eq!(plan.players_to_save[1].team_id, plan.outcome.away_team_id);

    // 既存に統合した選手の ID が fact の参照に使われる。
    let play = plan.facts.iter().find_map(|f| match &f.payload {
        handball_toolkit::facts::MatchFactPayload::Play(play) => Some(play),
        _ => None,
    });
    assert_eq!(play.unwrap().player_id, Some(existing_player_id));
}

/// ID 消費順は「新規 team（home → away）→ 新規 player（home → away）→ match →
/// factID 無し fact」。`required_import_id_count` はその総数と一致する。
#[test]
fn commit_plan_consumes_ids_in_documented_order() {
    let dto = default_import_dto();
    let decisions = ImportDecisions {
        home_team: TeamTarget::CreateNew,
        away_team: TeamTarget::CreateNew,
        players: HashMap::new(),
    };

    let required = required_import_id_count(&dto, &decisions);
    // 新規 team 2 + 新規 player 3 + match 1 + fallback fact 2 = 8。
    assert_eq!(required, 8);

    let plan = import_commit_plan(&dto, &decisions, sequential_ids()).unwrap();

    assert_eq!(plan.teams_to_save[0].id, TeamId(Uuid::from_u128(1))); // home team
    assert_eq!(plan.teams_to_save[1].id, TeamId(Uuid::from_u128(2))); // away team
    assert_eq!(plan.players_to_save[0].id, PlayerId(Uuid::from_u128(3))); // h1
    assert_eq!(plan.players_to_save[1].id, PlayerId(Uuid::from_u128(4))); // h2
    assert_eq!(plan.players_to_save[2].id, PlayerId(Uuid::from_u128(5))); // a1
    assert_eq!(plan.r#match.id.0, Uuid::from_u128(6));
    assert_eq!(plan.facts[0].id.0, Uuid::from_u128(7));
    assert_eq!(plan.facts[1].id.0, Uuid::from_u128(8));
}

/// 既存に統合した分だけ必要 ID 数が減る。
#[test]
fn required_id_count_excludes_reused_entities() {
    let dto = default_import_dto();
    let decisions = ImportDecisions {
        home_team: TeamTarget::Existing {
            team_id: TeamId(Uuid::from_u128(0xAA)),
        },
        away_team: TeamTarget::CreateNew,
        players: HashMap::from([(
            "h1".to_owned(),
            PlayerTarget::Existing {
                player_id: PlayerId(Uuid::from_u128(0xBB)),
            },
        )]),
    };

    // 新規 team 1 + 新規 player 2 + match 1 + fallback fact 2 = 6。
    assert_eq!(required_import_id_count(&dto, &decisions), 6);
}

/// 未知の playerKey を参照する fact があると decode error になる。
#[test]
fn commit_plan_rejects_unknown_fact_key() {
    let dto = import_dto(vec![play_fact_dto(Some("ghost"))]);
    let decisions = ImportDecisions {
        home_team: TeamTarget::CreateNew,
        away_team: TeamTarget::CreateNew,
        players: HashMap::new(),
    };

    let result = import_commit_plan(&dto, &decisions, sequential_ids());

    assert!(result.is_err());
}

// ── import_commit_batch（atomic 発火バッチの検証 + 組立 — handball-project#83）──

/// 検証を通る計画は、entity（新規のみ）と永続化順の facts をそのままバッチへ引き継ぐ。
/// バッチ発火（`commit_import`）は 1 回で、そこが 1 `context.save()` = atomic になる。
#[test]
fn import_commit_batch_assembles_batch_from_valid_plan() {
    let dto = default_import_dto();
    let decisions = ImportDecisions {
        home_team: TeamTarget::CreateNew,
        away_team: TeamTarget::CreateNew,
        players: HashMap::new(),
    };
    let plan = import_commit_plan(&dto, &decisions, sequential_ids()).unwrap();
    let expected_teams = plan.teams_to_save.clone();
    let expected_players = plan.players_to_save.clone();
    let expected_match = plan.r#match.clone();
    let expected_facts = plan.facts.clone();

    // 両チーム新規なので import 先の既存 roster は空。
    let batch = import_commit_batch(plan, &[]).expect("検証を通るはず");

    assert_eq!(batch.teams, expected_teams);
    assert_eq!(batch.players, expected_players);
    assert_eq!(batch.match_, expected_match);
    // facts は plan の永続化順のまま（並べ替えない）。
    assert_eq!(batch.facts, expected_facts);
}

/// いずれかの fact がプレフィックス検証に落ちたら `Err` を返し、バッチは 1 件も組まない。
/// = 呼び出し側は `commit_import` を発火しない = 何も保存されない（atomic の片側の保証）。
#[test]
fn import_commit_batch_rejects_invalid_sequence_without_assembling() {
    // phaseStart 無しで play だけ → timer の whole-log 検証（R3 / R5）で落ちる。
    let dto = import_dto(vec![play_fact_dto(Some("h1"))]);
    let decisions = ImportDecisions {
        home_team: TeamTarget::CreateNew,
        away_team: TeamTarget::CreateNew,
        players: HashMap::new(),
    };
    let plan = import_commit_plan(&dto, &decisions, sequential_ids()).unwrap();

    let result = import_commit_batch(plan, &[]);

    assert!(
        result.is_err(),
        "検証に落ちたらバッチを組まない（commit_import を発火しない）"
    );
    assert!(!result.unwrap_err().is_empty(), "issues は非空で返す");
}
