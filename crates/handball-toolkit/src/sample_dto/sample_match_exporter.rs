//! SAMPLE_DTO_V2 の export（domain → DTO）。オラクル: アプリ層 `SampleMatches/V2/MatchExporterV2.swift`。
//!
//! converter（P6 移植）と違い**新規実装**（ADR 0004 決定 2 — DTO 変換層の双方向 Rust 一本化）。
//! パリティ検証の枠外のため、Swift 実装の encode 出力そのもの（`tests/golden/export/`）との
//! バイト一致 + コーパス round-trip（`tests/sample_match_exporter_tests.rs`）で挙動を担保する。
//! 挙動は「改善」せず Swift に一致させる: playerKey は UUID **大文字**表記、fact は
//! `recordedAt` 昇順 stable sort、未知の team / player ID は `flatMap` 同様に黙ってキー省略。

use std::collections::BTreeMap;

use crate::clock::FactAnchor;
use crate::configuration::{MatchConfiguration, VideoProvider, VideoSource};
use crate::entities::{Match, Player, Team};
use crate::facts::{ControlFact, MatchFact, MatchFactPayload, PlayFact};
use crate::ids::{PlayerId, TeamId};

use super::sample_match_converter::{phase_kind_raw, play_event_kind_raw, stoppage_kind_raw};
use super::sample_match_dtos::{
    SCHEMA_VERSION_CURRENT, SampleControlFactDtoV2, SampleFactAnchorDtoV2, SampleFactDtoV2,
    SampleFactPayloadDtoV2, SampleMatchClockDtoV2, SampleMatchConfigurationDtoV2, SampleMatchDtoV2,
    SampleMatchHeaderV2, SamplePhaseStartPayloadDtoV2, SamplePlayFactDtoV2, SamplePlayerDtoV2,
    SampleStoppagePayloadDtoV2, SampleTeamDtoV2, SampleTeamsDtoV2, SampleTimerConfigurationDtoV2,
    SampleVideoClockDtoV2, SampleVideoConfigurationDtoV2, SampleVideoSourceDtoV2,
};

/// domain → DTO。Swift `MatchExporterV2.makeDTO` 相当の純粋関数。
///
/// 入力は repository から取得した中間値の組（Swift シグネチャの保存）。失敗しない —
/// validation は行わず、参照の解けない ID はキー省略で encode する（オラクル挙動）。
pub fn export_match(
    match_: &Match,
    home_team: &Team,
    away_team: &Team,
    home_players: &[Player],
    away_players: &[Player],
    facts: &[MatchFact],
) -> SampleMatchDtoV2 {
    const HOME_KEY: &str = "home";
    const AWAY_KEY: &str = "away";

    let home_dto = make_team_dto(HOME_KEY, home_team, home_players);
    let away_dto = make_team_dto(AWAY_KEY, away_team, away_players);

    let mut team_key_by_id: BTreeMap<TeamId, &str> = BTreeMap::new();
    team_key_by_id.insert(home_team.id, HOME_KEY);
    team_key_by_id.insert(away_team.id, AWAY_KEY);
    let player_key_by_id: BTreeMap<PlayerId, String> = home_players
        .iter()
        .chain(away_players)
        .map(|player| (player.id, uuid_key(player.id)))
        .collect();

    let header = SampleMatchHeaderV2 {
        display_name: match_.title.clone(),
        date: match_.date,
        configuration: encode_configuration(&match_.configuration),
    };

    let mut sorted_facts: Vec<&MatchFact> = facts.iter().collect();
    // stable sort — 同時刻の fact は入力順を保存する（Swift の sorted(by:) と同挙動）。
    sorted_facts.sort_by_key(|fact| fact.recorded_at);
    let fact_dtos = sorted_facts
        .into_iter()
        .map(|fact| SampleFactDtoV2 {
            fact_id: Some(fact.id),
            recorded_at: fact.recorded_at,
            payload: encode_payload(&fact.payload, &team_key_by_id, &player_key_by_id),
        })
        .collect();

    SampleMatchDtoV2 {
        schema_version: SCHEMA_VERSION_CURRENT,
        r#match: header,
        teams: SampleTeamsDtoV2 {
            home: home_dto,
            away: away_dto,
        },
        facts: fact_dtos,
    }
}

/// `2025-12-20-tigers-vs-falcons` のような ASCII slug（ファイル名向け）。
/// Swift `MatchExporterV2.defaultSlug` 相当。日付は UTC。
pub fn default_slug(match_: &Match, home_team: &Team, away_team: &Team) -> String {
    let date = match_.date.format("%Y-%m-%d").to_string();
    let home = ascii_slug(&home_team.name);
    let away = ascii_slug(&away_team.name);
    if !home.is_empty() && !away.is_empty() {
        return format!("{date}-{home}-vs-{away}");
    }
    let short_id: String = match_.id.to_string().chars().take(8).collect();
    format!("{date}-{short_id}")
}

// ── Teams / Players ──

fn make_team_dto(key: &str, team: &Team, players: &[Player]) -> SampleTeamDtoV2 {
    SampleTeamDtoV2 {
        key: key.to_owned(),
        name: team.name.clone(),
        players: players
            .iter()
            .map(|player| SamplePlayerDtoV2 {
                key: uuid_key(player.id),
                name: player.name.clone(),
                jersey_number: player.jersey_number,
            })
            .collect(),
    }
}

/// Swift `UUID.uuidString` 相当（大文字ハイフン表記）。playerKey の生成規則。
fn uuid_key(id: PlayerId) -> String {
    id.to_string().to_uppercase()
}

// ── Configuration ──

fn encode_configuration(configuration: &MatchConfiguration) -> SampleMatchConfigurationDtoV2 {
    match configuration {
        MatchConfiguration::Timer {
            phase_duration_seconds,
        } => SampleMatchConfigurationDtoV2 {
            kind: "timer".to_owned(),
            timer: Some(SampleTimerConfigurationDtoV2 {
                phase_duration_seconds: *phase_duration_seconds,
            }),
            video: None,
            video_highlight: None,
        },
        MatchConfiguration::Video(source) => SampleMatchConfigurationDtoV2 {
            kind: "video".to_owned(),
            timer: None,
            video: Some(SampleVideoConfigurationDtoV2 {
                source: encode_video_source(source),
            }),
            video_highlight: None,
        },
        MatchConfiguration::VideoHighlight(source) => SampleMatchConfigurationDtoV2 {
            kind: "videoHighlight".to_owned(),
            timer: None,
            video: None,
            video_highlight: Some(SampleVideoConfigurationDtoV2 {
                source: encode_video_source(source),
            }),
        },
    }
}

fn encode_video_source(source: &VideoSource) -> SampleVideoSourceDtoV2 {
    SampleVideoSourceDtoV2 {
        provider: match source.provider {
            VideoProvider::Youtube => "youtube".to_owned(),
            // round-trip（自分のテスト用 export → import）で .local も往復再現する（converter 参照）。
            VideoProvider::Local => "local".to_owned(),
        },
        external_id: source.external_id.clone(),
    }
}

// ── Fact payload ──

fn encode_payload(
    payload: &MatchFactPayload,
    team_key_by_id: &BTreeMap<TeamId, &str>,
    player_key_by_id: &BTreeMap<PlayerId, String>,
) -> SampleFactPayloadDtoV2 {
    match payload {
        MatchFactPayload::Play(play) => SampleFactPayloadDtoV2 {
            kind: "play".to_owned(),
            play: Some(encode_play(play, team_key_by_id, player_key_by_id)),
            control: None,
        },
        MatchFactPayload::Control(control) => SampleFactPayloadDtoV2 {
            kind: "control".to_owned(),
            play: None,
            control: Some(encode_control(control)),
        },
    }
}

fn encode_play(
    play: &PlayFact,
    team_key_by_id: &BTreeMap<TeamId, &str>,
    player_key_by_id: &BTreeMap<PlayerId, String>,
) -> SamplePlayFactDtoV2 {
    SamplePlayFactDtoV2 {
        kind: play_event_kind_raw(play.kind).to_owned(),
        team_key: play
            .team_id
            .and_then(|id| team_key_by_id.get(&id).map(|key| (*key).to_owned())),
        player_key: play
            .player_id
            .and_then(|id| player_key_by_id.get(&id).cloned()),
        related_player_key: play
            .related_player_id
            .and_then(|id| player_key_by_id.get(&id).cloned()),
        anchor: encode_anchor(play.anchor, None),
        title: play.title.clone(),
        note: play.note.clone(),
    }
}

fn encode_control(control: &ControlFact) -> SampleControlFactDtoV2 {
    match control {
        ControlFact::PhaseStart(payload) => SampleControlFactDtoV2 {
            kind: "phaseStart".to_owned(),
            phase_start: Some(SamplePhaseStartPayloadDtoV2 {
                kind: phase_kind_raw(payload.kind).to_owned(),
            }),
            stoppage: None,
            anchor: encode_anchor(payload.start_anchor, Some(payload.end_anchor)),
        },
        ControlFact::Stoppage(payload) => SampleControlFactDtoV2 {
            kind: "stoppage".to_owned(),
            phase_start: None,
            stoppage: Some(SampleStoppagePayloadDtoV2 {
                stoppage_kind: stoppage_kind_raw(payload.kind).to_owned(),
                note: payload.note.clone(),
            }),
            anchor: encode_anchor(payload.start_anchor, payload.end_anchor),
        },
    }
}

// ── Anchor ──

/// start anchor の kind に応じてフィールドを埋め、end anchor は kind を継承して
/// `endMatchElapsedSeconds` / `endVideoElapsedSeconds` に flatten する（converter の逆）。
fn encode_anchor(anchor: FactAnchor, end_anchor: Option<FactAnchor>) -> SampleFactAnchorDtoV2 {
    let (kind, match_clock, video_clock) = match anchor {
        FactAnchor::MatchClock(mc) => (
            "matchClock",
            Some(SampleMatchClockDtoV2 {
                elapsed_seconds: mc.elapsed_seconds,
            }),
            None,
        ),
        FactAnchor::VideoClock(vc) => (
            "videoClock",
            None,
            Some(SampleVideoClockDtoV2 {
                elapsed_seconds: vc.elapsed_seconds,
            }),
        ),
        FactAnchor::Both {
            match_clock,
            video_clock,
        } => (
            "both",
            Some(SampleMatchClockDtoV2 {
                elapsed_seconds: match_clock.elapsed_seconds,
            }),
            Some(SampleVideoClockDtoV2 {
                elapsed_seconds: video_clock.elapsed_seconds,
            }),
        ),
    };

    let (end_match, end_video) = match end_anchor {
        None => (None, None),
        Some(FactAnchor::MatchClock(mc)) => (Some(mc.elapsed_seconds), None),
        Some(FactAnchor::VideoClock(vc)) => (None, Some(vc.elapsed_seconds)),
        Some(FactAnchor::Both {
            match_clock,
            video_clock,
        }) => (
            Some(match_clock.elapsed_seconds),
            Some(video_clock.elapsed_seconds),
        ),
    };

    SampleFactAnchorDtoV2 {
        kind: kind.to_owned(),
        match_clock,
        video_clock,
        end_match_elapsed_seconds: end_match,
        end_video_elapsed_seconds: end_video,
    }
}

// ── Slug helpers ──

/// 小文字化 → ASCII 英小文字・数字以外を `-` に写像 → 連続 `-` を折り畳み。
/// Swift `asciiSlug` の unicodeScalars 走査と同値（Rust の `char` = Unicode scalar value）。
fn ascii_slug(raw: &str) -> String {
    let lowered = raw.to_lowercase();
    let mapped: String = lowered
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    mapped
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
