//! 移植元: `HandballRecorderTests/SampleMatchConverterV2Tests.swift`（アプリ層 — 分母 140 の外）。
//!
//! sample_dto converter（DTO → domain）の忠実性・拒否系テスト。
//! 配信 JSON の取込経路の中核なので、全 PlayEventKind / configuration 3 case / anchor 累積秒 /
//! 未知 key・kind の拒否 / PhaseStart end 必須 / Stoppage end 継承を固定する。
//!
//! Swift の suite static（homeTeamID 等）は per-test の `keys()` に置き換える。
//! ID 供給はシェル注入設計（converter 参照）のため、テストでは `Uuid::new_v4` を渡す。

mod fixtures;

use std::collections::{BTreeMap, BTreeSet};

use handball_toolkit::clock::FactAnchor;
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use handball_toolkit::facts::{ControlFact, MatchFactPayload, PlayEventKind, StoppageKind};
use handball_toolkit::ids::{PlayerId, TeamId};
use handball_toolkit::sample_dto::{
    SampleControlFactDtoV2, SampleFactAnchorDtoV2, SampleFactDtoV2, SampleFactPayloadDtoV2,
    SampleMatchClockDtoV2, SampleMatchConfigurationDtoV2, SampleMatchDecodeErrorV2,
    SampleMatchDtoV2, SampleMatchHeaderV2, SamplePhaseStartPayloadDtoV2, SamplePlayFactDtoV2,
    SamplePlayerDtoV2, SampleStoppagePayloadDtoV2, SampleTeamDtoV2, SampleTeamsDtoV2,
    SampleTimerConfigurationDtoV2, SampleVideoClockDtoV2, SampleVideoConfigurationDtoV2,
    SampleVideoSourceDtoV2, convert, decode_configuration, decode_fact,
};
use uuid::Uuid;

struct Keys {
    home_team_id: TeamId,
    away_team_id: TeamId,
    alice_id: PlayerId,
    bob_id: PlayerId,
}

fn keys() -> Keys {
    Keys {
        home_team_id: TeamId(Uuid::new_v4()),
        away_team_id: TeamId(Uuid::new_v4()),
        alice_id: PlayerId(Uuid::new_v4()),
        bob_id: PlayerId(Uuid::new_v4()),
    }
}

fn teams_by_key(keys: &Keys) -> BTreeMap<String, TeamId> {
    BTreeMap::from([
        ("home".to_owned(), keys.home_team_id),
        ("away".to_owned(), keys.away_team_id),
    ])
}

fn players_by_key(keys: &Keys) -> BTreeMap<String, PlayerId> {
    BTreeMap::from([
        ("alice".to_owned(), keys.alice_id),
        ("bob".to_owned(), keys.bob_id),
    ])
}

// ── DTO builders（Swift の file-scope helpers 相当） ──

fn anchor_dto(
    kind: &str,
    match_seconds: Option<f64>,
    video_seconds: Option<f64>,
    end_match: Option<f64>,
    end_video: Option<f64>,
) -> SampleFactAnchorDtoV2 {
    SampleFactAnchorDtoV2 {
        kind: kind.to_owned(),
        match_clock: match_seconds.map(|elapsed_seconds| SampleMatchClockDtoV2 { elapsed_seconds }),
        video_clock: video_seconds.map(|elapsed_seconds| SampleVideoClockDtoV2 { elapsed_seconds }),
        end_match_elapsed_seconds: end_match,
        end_video_elapsed_seconds: end_video,
    }
}

/// Swift `playFactDTO` 相当（factID は既定で新規 UUID、recordedAt は epoch）。
/// relatedPlayerKey / title / note が要るテストは戻り値の payload を直接書き換える。
fn play_fact_dto(
    kind: &str,
    team_key: Option<&str>,
    player_key: Option<&str>,
    anchor: SampleFactAnchorDtoV2,
) -> SampleFactDtoV2 {
    SampleFactDtoV2 {
        fact_id: Some(Uuid::new_v4()),
        recorded_at: fixtures::epoch(),
        payload: SampleFactPayloadDtoV2 {
            kind: "play".to_owned(),
            play: Some(SamplePlayFactDtoV2 {
                kind: kind.to_owned(),
                team_key: team_key.map(str::to_owned),
                player_key: player_key.map(str::to_owned),
                related_player_key: None,
                anchor,
                title: None,
                note: None,
            }),
            control: None,
            possession: None,
        },
    }
}

/// Swift `controlFactDTO` 相当（factID は既定で新規 UUID、recordedAt は epoch）。
fn control_fact_dto(
    kind: &str,
    phase_start_kind: Option<&str>,
    stoppage_kind: Option<&str>,
    note: Option<&str>,
    anchor: SampleFactAnchorDtoV2,
) -> SampleFactDtoV2 {
    SampleFactDtoV2 {
        fact_id: Some(Uuid::new_v4()),
        recorded_at: fixtures::epoch(),
        payload: SampleFactPayloadDtoV2 {
            kind: "control".to_owned(),
            play: None,
            control: Some(SampleControlFactDtoV2 {
                kind: kind.to_owned(),
                phase_start: phase_start_kind.map(|kind| SamplePhaseStartPayloadDtoV2 {
                    kind: kind.to_owned(),
                }),
                stoppage: stoppage_kind.map(|stoppage_kind| SampleStoppagePayloadDtoV2 {
                    stoppage_kind: stoppage_kind.to_owned(),
                    note: note.map(str::to_owned),
                }),
                anchor,
            }),
            possession: None,
        },
    }
}

fn timer_config_dto(seconds: f64) -> SampleMatchConfigurationDtoV2 {
    SampleMatchConfigurationDtoV2 {
        kind: "timer".to_owned(),
        timer: Some(SampleTimerConfigurationDtoV2 {
            phase_duration_seconds: seconds,
        }),
        video: None,
        video_highlight: None,
    }
}

fn video_config_dto(provider: &str, external_id: &str) -> SampleMatchConfigurationDtoV2 {
    SampleMatchConfigurationDtoV2 {
        kind: "video".to_owned(),
        timer: None,
        video: Some(SampleVideoConfigurationDtoV2 {
            source: SampleVideoSourceDtoV2 {
                provider: provider.to_owned(),
                external_id: external_id.to_owned(),
            },
        }),
        video_highlight: None,
    }
}

fn video_highlight_config_dto(provider: &str, external_id: &str) -> SampleMatchConfigurationDtoV2 {
    SampleMatchConfigurationDtoV2 {
        kind: "videoHighlight".to_owned(),
        timer: None,
        video: None,
        video_highlight: Some(SampleVideoConfigurationDtoV2 {
            source: SampleVideoSourceDtoV2 {
                provider: provider.to_owned(),
                external_id: external_id.to_owned(),
            },
        }),
    }
}

/// Swift `matchDTO` 相当（homeKey / awayKey は Swift テストでも既定値のみ使用のため固定）。
fn match_dto(
    schema_version: i64,
    configuration: SampleMatchConfigurationDtoV2,
    home_players: &[(&str, &str, Option<i64>)],
    away_players: &[(&str, &str, Option<i64>)],
    facts: Vec<SampleFactDtoV2>,
) -> SampleMatchDtoV2 {
    fn players(list: &[(&str, &str, Option<i64>)]) -> Vec<SamplePlayerDtoV2> {
        list.iter()
            .map(|(key, name, jersey_number)| SamplePlayerDtoV2 {
                key: (*key).to_owned(),
                name: (*name).to_owned(),
                jersey_number: *jersey_number,
            })
            .collect()
    }
    SampleMatchDtoV2 {
        schema_version,
        r#match: SampleMatchHeaderV2 {
            display_name: Some("テスト試合".to_owned()),
            date: fixtures::epoch(),
            configuration,
        },
        teams: SampleTeamsDtoV2 {
            home: SampleTeamDtoV2 {
                key: "home".to_owned(),
                name: "ホーム".to_owned(),
                players: players(home_players),
            },
            away: SampleTeamDtoV2 {
                key: "away".to_owned(),
                name: "アウェイ".to_owned(),
                players: players(away_players),
            },
        },
        facts,
    }
}

// ── configuration tagged union ──

#[test]
fn decode_timer_configuration() {
    let config = decode_configuration(&timer_config_dto(1800.0)).unwrap();
    assert_eq!(
        config,
        MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0
        }
    );
}

#[test]
fn decode_video_configuration() {
    let config = decode_configuration(&video_config_dto("youtube", "abc123")).unwrap();
    assert_eq!(
        config,
        MatchConfiguration::Video(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "abc123".to_owned(),
        })
    );
}

#[test]
fn decode_video_highlight_configuration() {
    let config = decode_configuration(&video_highlight_config_dto("youtube", "xyz")).unwrap();
    assert_eq!(
        config,
        MatchConfiguration::VideoHighlight(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: "xyz".to_owned(),
        })
    );
}

#[test]
fn decode_unknown_configuration_kind_throws() {
    let dto = SampleMatchConfigurationDtoV2 {
        kind: "mystery".to_owned(),
        timer: None,
        video: None,
        video_highlight: None,
    };
    assert_eq!(
        decode_configuration(&dto).unwrap_err(),
        SampleMatchDecodeErrorV2::UnknownConfigurationKind("mystery".to_owned())
    );
}

#[test]
fn decode_timer_with_missing_payload_throws() {
    let dto = SampleMatchConfigurationDtoV2 {
        kind: "timer".to_owned(),
        timer: None,
        video: None,
        video_highlight: None,
    };
    assert_eq!(
        decode_configuration(&dto).unwrap_err(),
        SampleMatchDecodeErrorV2::MissingConfigurationPayload("timer".to_owned())
    );
}

#[test]
fn decode_unknown_video_provider_throws() {
    let dto = video_config_dto("vimeo", "x");
    assert_eq!(
        decode_configuration(&dto).unwrap_err(),
        SampleMatchDecodeErrorV2::UnknownVideoProvider("vimeo".to_owned())
    );
}

// ── PlayFact decode（全 kind / key 解決） ──

/// Swift の parameterized test（`arguments: PlayEventKind.allCases`）はループで移植する。
#[test]
fn decode_play_fact_for_each_kind() {
    let cases = [
        (PlayEventKind::Goal, "goal"),
        (PlayEventKind::ShotMissed, "shotMissed"),
        (PlayEventKind::FreeNote, "freeNote"),
        (PlayEventKind::YellowCard, "yellowCard"),
        (PlayEventKind::TwoMinuteSuspension, "twoMinuteSuspension"),
        (PlayEventKind::RedCard, "redCard"),
    ];
    // rawValue 対応表が allCases を網羅していることを担保
    assert_eq!(cases.map(|(kind, _)| kind), PlayEventKind::ALL_CASES);
    let keys = keys();
    for (kind, raw) in cases {
        let dto = play_fact_dto(
            raw,
            Some("home"),
            Some("alice"),
            anchor_dto("matchClock", Some(600.0), None, None, None),
        );
        let fact = decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4,
        )
        .unwrap();
        let MatchFactPayload::Play(play) = fact.payload else {
            panic!("expected play");
        };
        assert_eq!(play.kind, kind);
        assert_eq!(play.team_id, Some(keys.home_team_id));
        assert_eq!(play.player_id, Some(keys.alice_id));
    }
}

#[test]
fn decode_play_fact_with_related_player() {
    let keys = keys();
    let mut dto = play_fact_dto(
        "twoMinuteSuspension",
        Some("home"),
        Some("alice"),
        anchor_dto("matchClock", Some(900.0), None, None, None),
    );
    dto.payload.play.as_mut().unwrap().related_player_key = Some("bob".to_owned());
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Play(play) = fact.payload else {
        panic!("expected play");
    };
    assert_eq!(play.related_player_id, Some(keys.bob_id));
}

#[test]
fn decode_play_fact_with_title_and_note() {
    let keys = keys();
    let mut dto = play_fact_dto(
        "freeNote",
        None,
        None,
        anchor_dto("matchClock", Some(60.0), None, None, None),
    );
    {
        let play = dto.payload.play.as_mut().unwrap();
        play.title = Some("メモ".to_owned());
        play.note = Some("詳細".to_owned());
    }
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Play(play) = fact.payload else {
        panic!("expected play");
    };
    assert_eq!(play.title.as_deref(), Some("メモ"));
    assert_eq!(play.note.as_deref(), Some("詳細"));
}

// ── PlayFact decode の拒否系 ──

#[test]
fn decode_fact_unknown_team_key_throws() {
    let keys = keys();
    let dto = play_fact_dto(
        "goal",
        Some("ghost"),
        Some("alice"),
        anchor_dto("matchClock", Some(0.0), None, None, None),
    );
    assert_eq!(
        decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4
        )
        .unwrap_err(),
        SampleMatchDecodeErrorV2::UnknownTeamKey("ghost".to_owned())
    );
}

#[test]
fn decode_fact_unknown_player_key_throws() {
    let keys = keys();
    let dto = play_fact_dto(
        "goal",
        Some("home"),
        Some("ghost"),
        anchor_dto("matchClock", Some(0.0), None, None, None),
    );
    assert_eq!(
        decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4
        )
        .unwrap_err(),
        SampleMatchDecodeErrorV2::UnknownPlayerKey("ghost".to_owned())
    );
}

#[test]
fn decode_fact_unknown_play_kind_throws() {
    let keys = keys();
    let dto = play_fact_dto(
        "bogus",
        Some("home"),
        Some("alice"),
        anchor_dto("matchClock", Some(0.0), None, None, None),
    );
    assert_eq!(
        decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4
        )
        .unwrap_err(),
        SampleMatchDecodeErrorV2::UnknownPlayKind("bogus".to_owned())
    );
}

// ── anchor 累積秒の忠実性 ──

#[test]
fn decode_match_clock_anchor_faithful() {
    let keys = keys();
    let dto = play_fact_dto(
        "goal",
        Some("home"),
        Some("alice"),
        anchor_dto("matchClock", Some(1234.5), None, None, None),
    );
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Play(play) = fact.payload else {
        panic!("expected play");
    };
    let FactAnchor::MatchClock(clock) = play.anchor else {
        panic!("expected matchClock");
    };
    assert_eq!(clock.elapsed_seconds, 1234.5);
}

#[test]
fn decode_video_clock_anchor_faithful() {
    let keys = keys();
    let dto = play_fact_dto(
        "goal",
        Some("home"),
        Some("alice"),
        anchor_dto("videoClock", None, Some(987.5), None, None),
    );
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Play(play) = fact.payload else {
        panic!("expected play");
    };
    let FactAnchor::VideoClock(clock) = play.anchor else {
        panic!("expected videoClock");
    };
    assert_eq!(clock.elapsed_seconds, 987.5);
}

#[test]
fn decode_both_anchor_faithful() {
    let keys = keys();
    let dto = play_fact_dto(
        "goal",
        Some("home"),
        Some("alice"),
        anchor_dto("both", Some(600.0), Some(1200.0), None, None),
    );
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Play(play) = fact.payload else {
        panic!("expected play");
    };
    let FactAnchor::Both {
        match_clock,
        video_clock,
    } = play.anchor
    else {
        panic!("expected both");
    };
    assert_eq!(match_clock.elapsed_seconds, 600.0);
    assert_eq!(video_clock.elapsed_seconds, 1200.0);
}

/// matchClock kind なのに matchClock body が無ければ missingAnchorBody。
#[test]
fn decode_anchor_missing_body_throws() {
    let keys = keys();
    let dto = play_fact_dto(
        "goal",
        Some("home"),
        Some("alice"),
        SampleFactAnchorDtoV2 {
            kind: "matchClock".to_owned(),
            match_clock: None,
            video_clock: None,
            end_match_elapsed_seconds: None,
            end_video_elapsed_seconds: None,
        },
    );
    assert_eq!(
        decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4
        )
        .unwrap_err(),
        SampleMatchDecodeErrorV2::MissingAnchorBody("matchClock".to_owned())
    );
}

/// play の anchor に end フィールドを入れても range にならず start のみ採用（end は無視）。
#[test]
fn decode_play_fact_ignores_end_anchor_fields() {
    let keys = keys();
    let dto = play_fact_dto(
        "goal",
        Some("home"),
        Some("alice"),
        anchor_dto("matchClock", Some(600.0), None, Some(1800.0), None),
    );
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Play(play) = fact.payload else {
        panic!("expected play");
    };
    let FactAnchor::MatchClock(clock) = play.anchor else {
        panic!("expected matchClock");
    };
    assert_eq!(clock.elapsed_seconds, 600.0);
}

// ── PhaseStart（end 必須） ──

#[test]
fn decode_phase_start_with_end_anchor() {
    let keys = keys();
    let dto = control_fact_dto(
        "phaseStart",
        Some("regular"),
        None,
        None,
        anchor_dto("matchClock", Some(0.0), None, Some(1800.0), None),
    );
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Control(ControlFact::PhaseStart(payload)) = fact.payload else {
        panic!("expected phaseStart");
    };
    assert_eq!(payload.kind, PhaseKind::Regular);
    assert_eq!(
        payload
            .start_anchor
            .match_clock()
            .map(|clock| clock.elapsed_seconds),
        Some(0.0)
    );
    assert_eq!(
        payload
            .end_anchor
            .match_clock()
            .map(|clock| clock.elapsed_seconds),
        Some(1800.0)
    );
}

#[test]
fn decode_phase_start_missing_end_throws() {
    let keys = keys();
    // end 無し
    let dto = control_fact_dto(
        "phaseStart",
        Some("regular"),
        None,
        None,
        anchor_dto("matchClock", Some(0.0), None, None, None),
    );
    assert_eq!(
        decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4
        )
        .unwrap_err(),
        SampleMatchDecodeErrorV2::MissingPhaseStartEnd
    );
}

#[test]
fn decode_phase_start_unknown_kind_throws() {
    let keys = keys();
    let dto = control_fact_dto(
        "phaseStart",
        Some("overtime"),
        None,
        None,
        anchor_dto("matchClock", Some(0.0), None, Some(1800.0), None),
    );
    assert_eq!(
        decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4
        )
        .unwrap_err(),
        SampleMatchDecodeErrorV2::UnknownPhaseKind("overtime".to_owned())
    );
}

// ── Stoppage（end optional, start kind 継承） ──

#[test]
fn decode_stoppage_with_end_inherits_start_kind() {
    let keys = keys();
    let dto = control_fact_dto(
        "stoppage",
        None,
        Some("timeout"),
        None,
        anchor_dto("videoClock", None, Some(600.0), None, Some(660.0)),
    );
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Control(ControlFact::Stoppage(payload)) = fact.payload else {
        panic!("expected stoppage");
    };
    assert_eq!(payload.kind, StoppageKind::Timeout);
    assert_eq!(
        payload
            .start_anchor
            .video_clock()
            .map(|clock| clock.elapsed_seconds),
        Some(600.0)
    );
    assert_eq!(
        payload
            .end_anchor
            .and_then(|anchor| anchor.video_clock())
            .map(|clock| clock.elapsed_seconds),
        Some(660.0)
    );
}

#[test]
fn decode_stoppage_without_end_is_nil() {
    let keys = keys();
    // end 無し
    let dto = control_fact_dto(
        "stoppage",
        None,
        Some("timeout"),
        None,
        anchor_dto("matchClock", Some(600.0), None, None, None),
    );
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Control(ControlFact::Stoppage(payload)) = fact.payload else {
        panic!("expected stoppage");
    };
    assert!(payload.end_anchor.is_none());
}

#[test]
fn decode_stoppage_with_note_round_trips() {
    let keys = keys();
    let dto = control_fact_dto(
        "stoppage",
        None,
        Some("pause"),
        Some("怪我対応"),
        anchor_dto("videoClock", None, Some(800.0), None, Some(860.0)),
    );
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let MatchFactPayload::Control(ControlFact::Stoppage(payload)) = fact.payload else {
        panic!("expected stoppage");
    };
    assert_eq!(payload.kind, StoppageKind::Pause);
    assert_eq!(payload.note.as_deref(), Some("怪我対応"));
}

#[test]
fn decode_stoppage_unknown_kind_throws() {
    let keys = keys();
    let dto = control_fact_dto(
        "stoppage",
        None,
        Some("intermission"),
        None,
        anchor_dto("matchClock", Some(600.0), None, None, None),
    );
    assert_eq!(
        decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4
        )
        .unwrap_err(),
        SampleMatchDecodeErrorV2::UnknownStoppageKind("intermission".to_owned())
    );
}

/// start matchClock だが end が videoClock しか持たない → 種別不一致で missingAnchorBody。
#[test]
fn decode_stoppage_end_kind_mismatch_throws() {
    let keys = keys();
    let dto = control_fact_dto(
        "stoppage",
        None,
        Some("timeout"),
        None,
        anchor_dto("matchClock", Some(600.0), None, None, Some(660.0)),
    );
    assert_eq!(
        decode_fact(
            &dto,
            &teams_by_key(&keys),
            &players_by_key(&keys),
            Uuid::new_v4
        )
        .unwrap_err(),
        SampleMatchDecodeErrorV2::MissingAnchorBody("end.matchClock".to_owned())
    );
}

// ── factID 採番 / 保持 ──

#[test]
fn decode_fact_preserves_provided_id() {
    let keys = keys();
    let id = Uuid::new_v4();
    let mut dto = play_fact_dto(
        "goal",
        Some("home"),
        Some("alice"),
        anchor_dto("matchClock", Some(0.0), None, None, None),
    );
    dto.fact_id = Some(id);
    let fact = decode_fact(
        &dto,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    assert_eq!(fact.id.0, id);
}

#[test]
fn decode_fact_generates_distinct_ids_when_nil() {
    let keys = keys();
    let mut dto1 = play_fact_dto(
        "goal",
        Some("home"),
        Some("alice"),
        anchor_dto("matchClock", Some(0.0), None, None, None),
    );
    dto1.fact_id = None;
    let mut dto2 = dto1.clone();
    dto2.fact_id = None;
    let f1 = decode_fact(
        &dto1,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    let f2 = decode_fact(
        &dto2,
        &teams_by_key(&keys),
        &players_by_key(&keys),
        Uuid::new_v4,
    )
    .unwrap();
    assert_ne!(f1.id, f2.id);
}

// ── convert() 全体 ──

#[test]
fn convert_rejects_schema_version_mismatch() {
    let dto = match_dto(
        1,
        timer_config_dto(1800.0),
        &[("h1", "ホーム1", Some(1))],
        &[("a1", "アウェイ1", Some(1))],
        vec![],
    );
    assert_eq!(
        convert("s", &dto, None, Uuid::new_v4).unwrap_err(),
        SampleMatchDecodeErrorV2::SchemaVersionMismatch {
            found: 1,
            expected: 2
        }
    );
}

#[test]
fn convert_produces_distinct_team_and_player_ids() {
    let dto = match_dto(
        2,
        timer_config_dto(1800.0),
        &[("h1", "ホーム1", Some(1))],
        &[("a1", "アウェイ1", Some(1))],
        vec![],
    );
    let result = convert("s", &dto, None, Uuid::new_v4).unwrap();
    assert_ne!(result.home_team.id, result.away_team.id);
    assert_eq!(result.players.len(), 2);
    assert_eq!(
        result
            .players
            .iter()
            .map(|player| player.id)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
}

#[test]
fn convert_resolves_fact_keys_to_team_and_player_ids() {
    let dto = match_dto(
        2,
        timer_config_dto(1800.0),
        &[("h1", "ホーム1", Some(1))],
        &[("a1", "アウェイ1", Some(1))],
        vec![
            control_fact_dto(
                "phaseStart",
                Some("regular"),
                None,
                None,
                anchor_dto("matchClock", Some(0.0), None, Some(1800.0), None),
            ),
            play_fact_dto(
                "goal",
                Some("home"),
                Some("h1"),
                anchor_dto("matchClock", Some(600.0), None, None, None),
            ),
        ],
    );
    let result = convert("s", &dto, None, Uuid::new_v4).unwrap();
    let goal = result
        .facts
        .iter()
        .find_map(|fact| {
            if let MatchFactPayload::Play(play) = &fact.payload {
                Some(play)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(goal.team_id, Some(result.home_team.id));
    let home_player_id = result
        .players
        .iter()
        .find(|player| player.team_id == result.home_team.id)
        .map(|player| player.id);
    assert_eq!(goal.player_id, home_player_id);
}

#[test]
fn convert_honors_configuration_override() {
    // index に kind=video が混入していても override で videoHighlight に強制できる。
    let source = VideoSource {
        provider: VideoProvider::Youtube,
        external_id: "abc".to_owned(),
    };
    let dto = match_dto(
        2,
        video_config_dto("youtube", "abc"),
        &[("h1", "ホーム1", Some(1))],
        &[("a1", "アウェイ1", Some(1))],
        vec![],
    );
    let result = convert(
        "s",
        &dto,
        Some(MatchConfiguration::VideoHighlight(source.clone())),
        Uuid::new_v4,
    )
    .unwrap();
    assert_eq!(
        result.r#match.configuration,
        MatchConfiguration::VideoHighlight(source)
    );
}
