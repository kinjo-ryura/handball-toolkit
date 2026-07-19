//! sample_dto export 方向（新規実装 — ADR 0004 決定 2）の検証。
//!
//! 1. **オラクル byte 一致**: `tests/golden/export/` の Swift `MatchExporterV2` encode 出力と
//!    文字列一致。決定的試合の定義は fixture 生成側と 1:1 対応（`golden/export/README.md`）
//! 2. **コーパス round-trip**: 実配信 JSON を parse → convert → export し、ID 命名の付け替えを
//!    除いて元 DTO と一致（converter / exporter が互いに逆写像であることを実データで確認）
//! 3. **domain round-trip**: export → encode → parse → convert で domain 値が復元される
//!    （`.iso8601` の秒未満切り捨てという lossy 挙動も明示的に釘付けする）

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use handball_toolkit::entities::{Match, Player, RosterSelection, Team};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
use handball_toolkit::ids::{FactId, MatchId, PlayerId, TeamId};
use handball_toolkit::sample_dto::{
    SampleMatchConversionResult, SampleMatchDtoV2, convert, default_slug, encode_sample_match,
    export_match, required_id_count,
};
use uuid::Uuid;

fn u(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn ts(secs: i64, nanos: u32) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, nanos).unwrap()
}

// ── 決定的試合（fixture 生成側の main.swift と 1:1 対応） ──

struct ExportFixture {
    match_: Match,
    home_team: Team,
    away_team: Team,
    home_players: Vec<Player>,
    away_players: Vec<Player>,
    facts: Vec<MatchFact>,
}

impl ExportFixture {
    fn export(&self) -> SampleMatchDtoV2 {
        export_match(
            &self.match_,
            &self.home_team,
            &self.away_team,
            &self.home_players,
            &self.away_players,
            &self.facts,
        )
    }

    fn slug(&self) -> String {
        default_slug(&self.match_, &self.home_team, &self.away_team)
    }
}

fn player(id: &str, team_id: TeamId, name: &str, jersey_number: Option<i64>) -> Player {
    Player {
        id: PlayerId(u(id)),
        team_id,
        name: name.to_owned(),
        jersey_number,
        photo: None,
    }
}

fn fact(n: u32, secs: i64, nanos: u32, payload: MatchFactPayload) -> MatchFact {
    MatchFact {
        id: FactId(u(&format!("fac00000-0000-0000-0000-{n:012}"))),
        recorded_at: ts(secs, nanos),
        payload,
    }
}

fn play(
    kind: PlayEventKind,
    team_id: Option<TeamId>,
    player_id: Option<PlayerId>,
    related_player_id: Option<PlayerId>,
    anchor: FactAnchor,
    title: Option<&str>,
    note: Option<&str>,
) -> MatchFactPayload {
    MatchFactPayload::Play(PlayFact {
        kind,
        team_id,
        player_id,
        related_player_id,
        anchor,
        title: title.map(str::to_owned),
        note: note.map(str::to_owned),
    })
}

fn mc(seconds: f64) -> FactAnchor {
    FactAnchor::MatchClock(MatchClock {
        elapsed_seconds: seconds,
    })
}

fn vc(seconds: f64) -> FactAnchor {
    FactAnchor::VideoClock(VideoClock {
        elapsed_seconds: seconds,
    })
}

fn timer_fixture() -> ExportFixture {
    let home_team = Team {
        id: TeamId(u("11111111-1111-1111-1111-111111111111")),
        name: "Tigers".to_owned(),
    };
    let away_team = Team {
        id: TeamId(u("22222222-2222-2222-2222-222222222222")),
        name: "Falcons".to_owned(),
    };
    let p1 = player(
        "aaaaaaaa-0000-0000-0000-000000000001",
        home_team.id,
        "Alice",
        Some(7),
    );
    let p2 = player(
        "aaaaaaaa-0000-0000-0000-000000000002",
        home_team.id,
        "美咲/GK 選手",
        None,
    );
    let p3 = player(
        "bbbbbbbb-0000-0000-0000-000000000001",
        away_team.id,
        "Bob",
        Some(99),
    );

    let match_ = Match {
        id: MatchId(u("33333333-3333-3333-3333-333333333333")),
        title: Some("決勝 / ファイナル".to_owned()),
        date: ts(1_700_000_000, 500_000_000), // 秒未満切り捨ての釘付け
        home_team_id: home_team.id,
        away_team_id: away_team.id,
        configuration: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    };

    let mut facts = vec![
        fact(
            1,
            1_700_000_100,
            0,
            MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
                kind: PhaseKind::Regular,
                start_anchor: mc(0.0),
                end_anchor: mc(1800.0),
            })),
        ),
        fact(
            2,
            1_700_000_200,
            250_000_000,
            play(
                PlayEventKind::Goal,
                Some(home_team.id),
                Some(p1.id),
                None,
                mc(63.5),
                None,
                Some("ナイスシュート"),
            ),
        ),
        fact(
            3,
            1_700_000_300,
            0,
            play(
                PlayEventKind::ShotMissed,
                Some(away_team.id),
                Some(p3.id),
                Some(p1.id),
                mc(120.0),
                Some("セーブ/ブロック"),
                None,
            ),
        ),
        fact(
            4,
            1_700_000_400,
            0,
            MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
                kind: StoppageKind::Timeout,
                start_anchor: mc(300.0),
                end_anchor: Some(mc(360.0)),
                note: None,
            })),
        ),
        fact(
            5,
            1_700_000_500,
            0,
            MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
                kind: StoppageKind::Pause,
                start_anchor: mc(400.0),
                end_anchor: Some(mc(460.0)),
                note: Some("給水/休憩".to_owned()),
            })),
        ),
        fact(
            6,
            1_700_000_600,
            0,
            play(
                PlayEventKind::FreeNote,
                None,
                None,
                None,
                mc(500.75),
                Some("メモ"),
                None,
            ),
        ),
        fact(
            7,
            1_700_000_700,
            0,
            play(
                PlayEventKind::YellowCard,
                Some(home_team.id),
                Some(p2.id),
                None,
                mc(600.0),
                None,
                None,
            ),
        ),
        fact(
            8,
            1_700_000_800,
            0,
            play(
                PlayEventKind::TwoMinuteSuspension,
                Some(away_team.id),
                Some(p3.id),
                None,
                mc(700.0),
                None,
                None,
            ),
        ),
        fact(
            9,
            1_700_000_900,
            0,
            play(
                PlayEventKind::RedCard,
                Some(home_team.id),
                Some(p1.id),
                None,
                mc(800.0),
                None,
                None,
            ),
        ),
        fact(
            10,
            1_700_001_000,
            0,
            MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
                kind: PhaseKind::Shootout,
                start_anchor: mc(3600.0),
                end_anchor: mc(3900.0),
            })),
        ),
    ];
    facts.reverse(); // 故意に逆順 → exporter の recordedAt 昇順ソートを釘付け

    ExportFixture {
        match_,
        home_team,
        away_team,
        home_players: vec![p1, p2],
        away_players: vec![p3],
        facts,
    }
}

fn video_fixture() -> ExportFixture {
    let home_team = Team {
        id: TeamId(u("44444444-4444-4444-4444-444444444444")),
        name: "湘南ブルー".to_owned(),
    };
    let away_team = Team {
        id: TeamId(u("55555555-5555-5555-5555-555555555555")),
        name: "横浜グリーン".to_owned(),
    };
    let p4 = player(
        "cccccccc-0000-0000-0000-000000000001",
        home_team.id,
        "Carol",
        Some(1),
    );

    let match_ = Match {
        id: MatchId(u("66666666-6666-6666-6666-666666666666")),
        title: None, // displayName キー省略の釘付け
        date: ts(1_750_000_000, 0),
        home_team_id: home_team.id,
        away_team_id: away_team.id,
        configuration: MatchConfiguration::Video(VideoSource {
            provider: VideoProvider::Local,
            external_id: "phasset-ABC123/xyz".to_owned(),
        }),
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    };

    let mut facts = vec![
        fact(
            11,
            1_750_000_010,
            0,
            MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
                kind: PhaseKind::Regular,
                start_anchor: vc(10.0),
                end_anchor: vc(70.0),
            })),
        ),
        fact(
            12,
            1_750_000_020,
            750_000_000,
            play(
                PlayEventKind::Goal,
                Some(home_team.id),
                Some(p4.id),
                None,
                vc(30.25),
                None,
                None,
            ),
        ),
        fact(
            13,
            1_750_000_030,
            0,
            MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
                kind: StoppageKind::Timeout,
                start_anchor: FactAnchor::Both {
                    match_clock: MatchClock {
                        elapsed_seconds: 15.0,
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: 25.0,
                    },
                },
                end_anchor: Some(FactAnchor::Both {
                    match_clock: MatchClock {
                        elapsed_seconds: 15.0,
                    },
                    video_clock: VideoClock {
                        elapsed_seconds: 40.0,
                    },
                }),
                note: None,
            })),
        ),
        fact(
            14,
            1_750_000_040,
            0,
            play(
                PlayEventKind::Goal,
                Some(away_team.id),
                None,
                None,
                vc(50.0),
                None,
                None,
            ),
        ),
    ];
    facts.reverse();

    ExportFixture {
        match_,
        home_team,
        away_team,
        home_players: vec![p4],
        away_players: Vec::new(), // 空配列の書式釘付け
        facts,
    }
}

fn video_highlight_fixture() -> ExportFixture {
    let home_team = Team {
        id: TeamId(u("77777777-7777-7777-7777-777777777777")),
        name: "Osaka 365 Stars".to_owned(),
    };
    let away_team = Team {
        id: TeamId(u("88888888-8888-8888-8888-888888888888")),
        name: "Blue Owls".to_owned(),
    };
    let p5 = player(
        "dddddddd-0000-0000-0000-000000000001",
        home_team.id,
        "Daisuke",
        Some(10),
    );
    let p6 = player(
        "eeeeeeee-0000-0000-0000-000000000001",
        away_team.id,
        "Emi",
        None,
    );

    let match_ = Match {
        id: MatchId(u("99999999-9999-9999-9999-999999999999")),
        title: Some("ハイライト集".to_owned()),
        date: ts(1_760_000_000, 999_000_000), // .999 も切り捨て（丸め上げしない）の釘付け
        home_team_id: home_team.id,
        away_team_id: away_team.id,
        configuration: MatchConfiguration::VideoHighlight(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "dQw4w9WgXcQ".to_owned(),
        }),
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    };

    ExportFixture {
        match_,
        home_team,
        away_team,
        home_players: vec![p5],
        away_players: vec![p6],
        facts: Vec::new(), // facts 0 件の書式釘付け
    }
}

// ── 1. オラクル byte 一致 ──

#[test]
fn timer_export_matches_swift_oracle_bytes() {
    let dto = timer_fixture().export();
    assert_eq!(
        encode_sample_match(&dto),
        include_str!("golden/export/timer.json")
    );
}

#[test]
fn video_export_matches_swift_oracle_bytes() {
    let dto = video_fixture().export();
    assert_eq!(
        encode_sample_match(&dto),
        include_str!("golden/export/video.json")
    );
}

#[test]
fn video_highlight_export_matches_swift_oracle_bytes() {
    let dto = video_highlight_fixture().export();
    assert_eq!(
        encode_sample_match(&dto),
        include_str!("golden/export/video-highlight.json")
    );
}

#[test]
fn default_slug_matches_swift_oracle() {
    let slugs: BTreeMap<String, String> =
        serde_json::from_str(include_str!("golden/export/slugs.json")).unwrap();
    assert_eq!(timer_fixture().slug(), slugs["timer"]);
    assert_eq!(video_fixture().slug(), slugs["video"]);
    assert_eq!(video_highlight_fixture().slug(), slugs["video-highlight"]);
}

// ── 2. コーパス round-trip ──

fn corpus_input_paths() -> Vec<PathBuf> {
    let golden_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut roots = vec![golden_dir.join("inputs")];
    let local_inputs = golden_dir.join("local/inputs");
    if local_inputs.is_dir() {
        roots.push(local_inputs); // ローカル .timer コーパスがあれば含める（無い環境はスキップ）
    }

    let mut paths = Vec::new();
    for root in roots {
        for group in fs::read_dir(&root).unwrap() {
            let group = group.unwrap().path();
            if !group.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&group).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                    paths.push(path);
                }
            }
        }
    }
    paths.sort();
    paths
}

/// exporter が新規採番した ID ベースのキーを、convert の逆写像で元コーパスのキーへ戻す。
fn normalize_exported_keys(
    mut exported: SampleMatchDtoV2,
    original: &SampleMatchDtoV2,
    conversion: &SampleMatchConversionResult,
) -> SampleMatchDtoV2 {
    let player_key_map: BTreeMap<String, String> = conversion
        .players_by_key
        .iter()
        .map(|(orig_key, id)| (id.to_string().to_uppercase(), orig_key.clone()))
        .collect();
    let team_key_map: BTreeMap<String, String> = BTreeMap::from([
        ("home".to_owned(), original.teams.home.key.clone()),
        ("away".to_owned(), original.teams.away.key.clone()),
    ]);
    let remap_player = |key: &mut Option<String>| {
        if let Some(value) = key {
            *value = player_key_map
                .get(value)
                .unwrap_or_else(|| panic!("未知の playerKey: {value}"))
                .clone();
        }
    };

    exported.teams.home.key = original.teams.home.key.clone();
    exported.teams.away.key = original.teams.away.key.clone();
    for player_dto in exported
        .teams
        .home
        .players
        .iter_mut()
        .chain(exported.teams.away.players.iter_mut())
    {
        player_dto.key = player_key_map[&player_dto.key].clone();
    }
    for fact_dto in &mut exported.facts {
        if let Some(play_dto) = &mut fact_dto.payload.play {
            if let Some(team_key) = &mut play_dto.team_key {
                *team_key = team_key_map[team_key.as_str()].clone();
            }
            remap_player(&mut play_dto.player_key);
            remap_player(&mut play_dto.related_player_key);
        }
    }
    exported
}

#[test]
fn corpus_round_trip_export_restores_original_dto() {
    let inputs = corpus_input_paths();
    assert!(!inputs.is_empty(), "コーパスが見つからない");

    for path in &inputs {
        let json = fs::read_to_string(path).unwrap();
        let dto: SampleMatchDtoV2 = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("{}: parse 失敗: {error}", path.display()));

        // convert（ID は決定的な連番で供給し、消費数が required_id_count と一致することも確認）
        let mut counter: u128 = 0;
        let conversion = convert("round-trip", &dto, None, || {
            counter += 1;
            Uuid::from_u128(counter)
        })
        .unwrap_or_else(|error| panic!("{}: convert 失敗: {error:?}", path.display()));
        assert_eq!(
            counter as usize,
            required_id_count(&dto),
            "{}: required_id_count と実消費数の不一致",
            path.display()
        );

        // export（players は team_id で home / away に振り分け — DTO 出現順が保たれる）
        let home_players: Vec<Player> = conversion
            .players
            .iter()
            .filter(|p| p.team_id == conversion.home_team.id)
            .cloned()
            .collect();
        let away_players: Vec<Player> = conversion
            .players
            .iter()
            .filter(|p| p.team_id == conversion.away_team.id)
            .cloned()
            .collect();
        let exported = export_match(
            &conversion.r#match,
            &conversion.home_team,
            &conversion.away_team,
            &home_players,
            &away_players,
            &conversion.facts,
        );

        // 期待値: 元 DTO の facts を recordedAt 昇順（stable）に整列したもの
        let mut expected = dto.clone();
        expected.facts.sort_by_key(|fact| fact.recorded_at);

        // 正規化: ID 由来キーを元キーへ戻し、元が factID 無しの fact は採番を消す
        let mut normalized = normalize_exported_keys(exported, &dto, &conversion);
        for (normalized_fact, expected_fact) in normalized.facts.iter_mut().zip(&expected.facts) {
            if expected_fact.fact_id.is_none() {
                normalized_fact.fact_id = None;
            }
        }

        assert_eq!(
            normalized,
            expected,
            "{}: round-trip が元 DTO と不一致",
            path.display()
        );
    }
    println!("corpus round-trip: {} 件検証", inputs.len());
}

// ── 3. domain round-trip（encode → parse → convert） ──

#[test]
fn encode_parse_convert_round_trip_restores_domain() {
    let fixture = timer_fixture();
    let encoded = encode_sample_match(&fixture.export());
    let parsed: SampleMatchDtoV2 = serde_json::from_str(&encoded).unwrap();

    let mut counter: u128 = 0;
    let conversion = convert("round-trip", &parsed, None, || {
        counter += 1;
        Uuid::from_u128(counter)
    })
    .unwrap();

    // factID は保存され、recordedAt 昇順に並ぶ
    let restored_ids: Vec<FactId> = conversion.facts.iter().map(|fact| fact.id).collect();
    let expected_ids: Vec<FactId> = (1..=10)
        .map(|n| FactId(u(&format!("fac00000-0000-0000-0000-{n:012}"))))
        .collect();
    assert_eq!(restored_ids, expected_ids);

    // recordedAt / date は秒未満が切り捨てられて復元される（Swift `.iso8601` のオラクル挙動。
    // 元: date .5 / goal の recordedAt .25 — lossy であることを明示的に釘付け）
    assert_eq!(conversion.r#match.date, ts(1_700_000_000, 0));
    assert_eq!(conversion.facts[1].recorded_at, ts(1_700_000_200, 0));

    // header / configuration / チーム・選手の属性は保持（ID は再採番）
    assert_eq!(
        conversion.r#match.title.as_deref(),
        Some("決勝 / ファイナル")
    );
    assert_eq!(
        conversion.r#match.configuration,
        MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0
        }
    );
    assert_eq!(conversion.home_team.name, "Tigers");
    assert_eq!(conversion.away_team.name, "Falcons");
    assert!(
        conversion
            .players
            .iter()
            .any(|p| p.name == "美咲/GK 選手" && p.jersey_number.is_none())
    );

    // goal の参照は再採番後の home 側 ID へ remap される
    let MatchFactPayload::Play(goal) = &conversion.facts[1].payload else {
        panic!("facts[1] は goal のはず");
    };
    assert_eq!(goal.kind, PlayEventKind::Goal);
    assert_eq!(goal.team_id, Some(conversion.home_team.id));
    let alice_key = "AAAAAAAA-0000-0000-0000-000000000001";
    assert_eq!(goal.player_id, Some(conversion.players_by_key[alice_key]));
    assert_eq!(goal.anchor, mc(63.5));
    assert_eq!(goal.note.as_deref(), Some("ナイスシュート"));
}
