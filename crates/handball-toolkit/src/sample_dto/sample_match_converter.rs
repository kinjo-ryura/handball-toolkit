//! 移植元: アプリ層 `SampleMatches/V2/SampleMatchConverterV2.swift`。
//!
//! V2 sample DTO を domain（Match / Team / Player / MatchFact）に変換する純粋関数群。
//! 試合内 teamKey / playerKey は試合ごとの新規 ID にマップし、試合をまたいだ衝突を回避する。
//!
//! Swift からの意図的乖離（ADR 0003 §2 追記）:
//! - **ID 生成の注入**: Swift の `UUID()` 直生成は設計不変条件（コアに ID 生成を置かない）に
//!   反するため、`new_id` closure をシェルから受け取る。呼び出し順は Swift の生成順を保存する
//!   （home team → home players → away team → away players → match → factID 無し fact の順）
//! - **逆写像の同梱**: 変換結果に teamKey / playerKey → 内部 ID の写像を含める
//!   （ADR 0003 §3「内部 ID → コーパスキーの逆写像」によるゴールデン正規化の材料）

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::clock::{FactAnchor, FactAnchorKind, MatchClock, VideoClock};
use crate::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use crate::entities::{Match, Player, RosterSelection, Team};
use crate::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PlayEventKind, PlayFact,
    StoppageKind, StoppagePayload,
};
use crate::ids::{FactId, MatchId, PlayerId, TeamId};

use super::sample_match_dtos::{
    SCHEMA_VERSION_CURRENT, SampleControlFactDtoV2, SampleFactAnchorDtoV2, SampleFactDtoV2,
    SampleFactPayloadDtoV2, SampleMatchConfigurationDtoV2, SampleMatchDecodeErrorV2,
    SampleMatchDtoV2, SamplePlayFactDtoV2, SampleTeamDtoV2, SampleVideoSourceDtoV2,
};

/// 1 試合分の変換結果。
///
/// `teams_by_key` / `players_by_key` は Rust 側の追加フィールド（ADR 0003 §3 のゴールデン
/// 正規化で「内部 ID → コーパスキー」の逆写像を作る材料に加え、FFI ではシェルの merge
/// 調停が既存 DB との突合に使う。Swift 版には無い）。
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SampleMatchConversionResult {
    pub r#match: Match,
    pub home_team: Team,
    pub away_team: Team,
    pub players: Vec<Player>,
    pub facts: Vec<MatchFact>,
    /// teamKey（`home` / `away`）→ 内部 ID。
    pub teams_by_key: BTreeMap<String, TeamId>,
    /// playerKey（JSON 内の文字列キー）→ 内部 ID。
    pub players_by_key: BTreeMap<String, PlayerId>,
}

impl SampleMatchConversionResult {
    /// Swift `var teams: [Team]` 相当。
    pub fn teams(&self) -> Vec<Team> {
        vec![self.home_team.clone(), self.away_team.clone()]
    }
}

/// `convert` が消費する新規 ID の数。
///
/// 内訳は生成順どおり: home team 1 + home 選手 + away team 1 + away 選手 + match 1 +
/// `factID` 無し fact。FFI の「事前生成 `Vec<Uuid>`」方式（ADR 0004 決定 2）でシェルが
/// 生成数を知るための関数で、消費順の知識をシェルへ漏らさないためコア側に置く。
pub fn required_id_count(dto: &SampleMatchDtoV2) -> usize {
    (1 + dto.teams.home.players.len())
        + (1 + dto.teams.away.players.len())
        + 1
        + dto
            .facts
            .iter()
            .filter(|fact| fact.fact_id.is_none())
            .count()
}

/// DTO → domain 変換。
///
/// - `_slug`: ログ・エラー文脈用（内部識別には使わない。Swift シグネチャの保存）。
/// - `configuration_override`: 経路側で `VideoHighlight` 等を強制したい場合。
///   None なら DTO 内の `configuration` をそのまま使う（Highlight 経路は通常 override で固定）。
/// - `new_id`: 新規 ID の供給源（シェル注入。テストでは決定的な列を渡せる）。
pub fn convert(
    _slug: &str,
    dto: &SampleMatchDtoV2,
    configuration_override: Option<MatchConfiguration>,
    mut new_id: impl FnMut() -> Uuid,
) -> Result<SampleMatchConversionResult, SampleMatchDecodeErrorV2> {
    if dto.schema_version != SCHEMA_VERSION_CURRENT {
        return Err(SampleMatchDecodeErrorV2::SchemaVersionMismatch {
            found: dto.schema_version,
            expected: SCHEMA_VERSION_CURRENT,
        });
    }

    let (home_team, home_players) = make_team(&dto.teams.home, &mut new_id);
    let (away_team, away_players) = make_team(&dto.teams.away, &mut new_id);

    let mut teams_by_key: BTreeMap<String, TeamId> = BTreeMap::new();
    teams_by_key.insert(dto.teams.home.key.clone(), home_team.id);
    teams_by_key.insert(dto.teams.away.key.clone(), away_team.id);

    let mut players_by_key: BTreeMap<String, PlayerId> = BTreeMap::new();
    for (key, player) in &home_players {
        players_by_key.insert(key.clone(), player.id);
    }
    for (key, player) in &away_players {
        players_by_key.insert(key.clone(), player.id);
    }

    let configuration = match configuration_override {
        Some(configuration) => configuration,
        None => decode_configuration(&dto.r#match.configuration)?,
    };

    let match_ = Match {
        id: MatchId(new_id()),
        title: dto.r#match.display_name.clone(),
        date: dto.r#match.date,
        home_team_id: home_team.id,
        away_team_id: away_team.id,
        configuration,
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    };

    let facts = dto
        .facts
        .iter()
        .map(|fact_dto| decode_fact(fact_dto, &teams_by_key, &players_by_key, &mut new_id))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SampleMatchConversionResult {
        r#match: match_,
        home_team,
        away_team,
        players: home_players
            .into_iter()
            .map(|(_, player)| player)
            .chain(away_players.into_iter().map(|(_, player)| player))
            .collect(),
        facts,
        teams_by_key,
        players_by_key,
    })
}

// ── Teams ──

fn make_team(
    dto: &SampleTeamDtoV2,
    new_id: &mut impl FnMut() -> Uuid,
) -> (Team, Vec<(String, Player)>) {
    let team = Team {
        id: TeamId(new_id()),
        name: dto.name.clone(),
    };
    let players = dto
        .players
        .iter()
        .map(|player_dto| {
            let player = Player {
                id: PlayerId(new_id()),
                team_id: team.id,
                name: player_dto.name.clone(),
                jersey_number: player_dto.jersey_number,
                photo: None,
            };
            (player_dto.key.clone(), player)
        })
        .collect();
    (team, players)
}

// ── Configuration ──

/// Configuration tagged union を decode する（Swift 同様、単体でも再利用可能な公開関数）。
pub fn decode_configuration(
    dto: &SampleMatchConfigurationDtoV2,
) -> Result<MatchConfiguration, SampleMatchDecodeErrorV2> {
    match dto.kind.as_str() {
        "timer" => {
            let Some(timer) = &dto.timer else {
                return Err(SampleMatchDecodeErrorV2::MissingConfigurationPayload(
                    "timer".to_owned(),
                ));
            };
            Ok(MatchConfiguration::Timer {
                phase_duration_seconds: timer.phase_duration_seconds,
            })
        }
        "video" => {
            let Some(video) = &dto.video else {
                return Err(SampleMatchDecodeErrorV2::MissingConfigurationPayload(
                    "video".to_owned(),
                ));
            };
            Ok(MatchConfiguration::Video(decode_video_source(
                &video.source,
            )?))
        }
        "videoHighlight" => {
            let Some(video) = &dto.video_highlight else {
                return Err(SampleMatchDecodeErrorV2::MissingConfigurationPayload(
                    "videoHighlight".to_owned(),
                ));
            };
            Ok(MatchConfiguration::VideoHighlight(decode_video_source(
                &video.source,
            )?))
        }
        _ => Err(SampleMatchDecodeErrorV2::UnknownConfigurationKind(
            dto.kind.clone(),
        )),
    }
}

fn decode_video_source(
    dto: &SampleVideoSourceDtoV2,
) -> Result<VideoSource, SampleMatchDecodeErrorV2> {
    match dto.provider.as_str() {
        "youtube" => Ok(VideoSource {
            provider: VideoProvider::Youtube,
            external_id: dto.external_id.clone(),
        }),
        // round-trip import で local を復元する（同一端末では localIdentifier 経由で動画ごと復元、
        // 別端末では参照が切れて再生時 error）。配布サンプル経路はこの decoder を通さず youtube-only。
        "local" => Ok(VideoSource {
            provider: VideoProvider::Local,
            external_id: dto.external_id.clone(),
        }),
        _ => Err(SampleMatchDecodeErrorV2::UnknownVideoProvider(
            dto.provider.clone(),
        )),
    }
}

// ── Facts ──

/// 1 fact DTO を domain `MatchFact` に decode する（teams_by_key / players_by_key を再利用）。
/// Swift 同様、単体でも再利用可能な公開関数。
pub fn decode_fact(
    dto: &SampleFactDtoV2,
    teams_by_key: &BTreeMap<String, TeamId>,
    players_by_key: &BTreeMap<String, PlayerId>,
    mut new_id: impl FnMut() -> Uuid,
) -> Result<MatchFact, SampleMatchDecodeErrorV2> {
    let id = match dto.fact_id {
        Some(id) => id,
        None => new_id(),
    };
    let payload = decode_payload(&dto.payload, teams_by_key, players_by_key)?;
    Ok(MatchFact {
        id: FactId(id),
        recorded_at: dto.recorded_at,
        payload,
    })
}

fn decode_payload(
    dto: &SampleFactPayloadDtoV2,
    teams_by_key: &BTreeMap<String, TeamId>,
    players_by_key: &BTreeMap<String, PlayerId>,
) -> Result<MatchFactPayload, SampleMatchDecodeErrorV2> {
    match dto.kind.as_str() {
        "play" => {
            let Some(play) = &dto.play else {
                return Err(SampleMatchDecodeErrorV2::MissingPayloadBody(
                    "play".to_owned(),
                ));
            };
            Ok(MatchFactPayload::Play(decode_play_fact(
                play,
                teams_by_key,
                players_by_key,
            )?))
        }
        "control" => {
            let Some(control) = &dto.control else {
                return Err(SampleMatchDecodeErrorV2::MissingPayloadBody(
                    "control".to_owned(),
                ));
            };
            Ok(MatchFactPayload::Control(decode_control_fact(control)?))
        }
        _ => Err(SampleMatchDecodeErrorV2::UnknownPayloadKind(
            dto.kind.clone(),
        )),
    }
}

fn decode_play_fact(
    dto: &SamplePlayFactDtoV2,
    teams_by_key: &BTreeMap<String, TeamId>,
    players_by_key: &BTreeMap<String, PlayerId>,
) -> Result<PlayFact, SampleMatchDecodeErrorV2> {
    let Some(kind) = play_event_kind_from_raw(&dto.kind) else {
        return Err(SampleMatchDecodeErrorV2::UnknownPlayKind(dto.kind.clone()));
    };
    let team_id = dto
        .team_key
        .as_ref()
        .map(|key| {
            teams_by_key
                .get(key)
                .copied()
                .ok_or_else(|| SampleMatchDecodeErrorV2::UnknownTeamKey(key.clone()))
        })
        .transpose()?;
    let player_id = dto
        .player_key
        .as_ref()
        .map(|key| {
            players_by_key
                .get(key)
                .copied()
                .ok_or_else(|| SampleMatchDecodeErrorV2::UnknownPlayerKey(key.clone()))
        })
        .transpose()?;
    let related_player_id = dto
        .related_player_key
        .as_ref()
        .map(|key| {
            players_by_key
                .get(key)
                .copied()
                .ok_or_else(|| SampleMatchDecodeErrorV2::UnknownPlayerKey(key.clone()))
        })
        .transpose()?;
    let anchor = decode_start_anchor(&dto.anchor)?;
    Ok(PlayFact {
        kind,
        team_id,
        player_id,
        related_player_id,
        anchor,
        title: dto.title.clone(),
        note: dto.note.clone(),
    })
}

fn decode_control_fact(
    dto: &SampleControlFactDtoV2,
) -> Result<ControlFact, SampleMatchDecodeErrorV2> {
    match dto.kind.as_str() {
        "phaseStart" => {
            let Some(phase_start) = &dto.phase_start else {
                return Err(SampleMatchDecodeErrorV2::MissingPayloadBody(
                    "phaseStart".to_owned(),
                ));
            };
            let Some(phase_kind) = phase_kind_from_raw(&phase_start.kind) else {
                return Err(SampleMatchDecodeErrorV2::UnknownPhaseKind(
                    phase_start.kind.clone(),
                ));
            };
            let start_anchor = decode_start_anchor(&dto.anchor)?;
            // PhaseStart は endAnchor 必須（domain 不変条件）。
            let Some(end_anchor) = decode_end_anchor(&dto.anchor, start_anchor)? else {
                return Err(SampleMatchDecodeErrorV2::MissingPhaseStartEnd);
            };
            Ok(ControlFact::PhaseStart(PhaseStartPayload {
                kind: phase_kind,
                start_anchor,
                end_anchor,
            }))
        }
        "stoppage" => {
            let Some(stoppage) = &dto.stoppage else {
                return Err(SampleMatchDecodeErrorV2::MissingPayloadBody(
                    "stoppage".to_owned(),
                ));
            };
            let Some(stoppage_kind) = stoppage_kind_from_raw(&stoppage.stoppage_kind) else {
                return Err(SampleMatchDecodeErrorV2::UnknownStoppageKind(
                    stoppage.stoppage_kind.clone(),
                ));
            };
            let start_anchor = decode_start_anchor(&dto.anchor)?;
            let end_anchor = decode_end_anchor(&dto.anchor, start_anchor)?;
            Ok(ControlFact::Stoppage(StoppagePayload {
                kind: stoppage_kind,
                start_anchor,
                end_anchor,
                note: stoppage.note.clone(),
            }))
        }
        _ => Err(SampleMatchDecodeErrorV2::UnknownControlKind(
            dto.kind.clone(),
        )),
    }
}

// ── Anchor ──

fn decode_start_anchor(
    dto: &SampleFactAnchorDtoV2,
) -> Result<FactAnchor, SampleMatchDecodeErrorV2> {
    match dto.kind.as_str() {
        "matchClock" => {
            let Some(match_clock) = &dto.match_clock else {
                return Err(SampleMatchDecodeErrorV2::MissingAnchorBody(
                    "matchClock".to_owned(),
                ));
            };
            Ok(FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: match_clock.elapsed_seconds,
            }))
        }
        "videoClock" => {
            let Some(video_clock) = &dto.video_clock else {
                return Err(SampleMatchDecodeErrorV2::MissingAnchorBody(
                    "videoClock".to_owned(),
                ));
            };
            Ok(FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: video_clock.elapsed_seconds,
            }))
        }
        "both" => {
            let Some(match_clock) = &dto.match_clock else {
                return Err(SampleMatchDecodeErrorV2::MissingAnchorBody(
                    "both.matchClock".to_owned(),
                ));
            };
            let Some(video_clock) = &dto.video_clock else {
                return Err(SampleMatchDecodeErrorV2::MissingAnchorBody(
                    "both.videoClock".to_owned(),
                ));
            };
            Ok(FactAnchor::Both {
                match_clock: MatchClock {
                    elapsed_seconds: match_clock.elapsed_seconds,
                },
                video_clock: VideoClock {
                    elapsed_seconds: video_clock.elapsed_seconds,
                },
            })
        }
        _ => Err(SampleMatchDecodeErrorV2::UnknownAnchorKind(
            dto.kind.clone(),
        )),
    }
}

/// end anchor を start anchor の種別から継承して構築する。
/// end フィールドが両方 None の場合は None を返す（range を持たない fact）。
fn decode_end_anchor(
    dto: &SampleFactAnchorDtoV2,
    start_anchor: FactAnchor,
) -> Result<Option<FactAnchor>, SampleMatchDecodeErrorV2> {
    let end_match = dto.end_match_elapsed_seconds;
    let end_video = dto.end_video_elapsed_seconds;
    if end_match.is_none() && end_video.is_none() {
        return Ok(None);
    }

    match start_anchor.kind() {
        FactAnchorKind::MatchClock => {
            let Some(end_match) = end_match else {
                return Err(SampleMatchDecodeErrorV2::MissingAnchorBody(
                    "end.matchClock".to_owned(),
                ));
            };
            Ok(Some(FactAnchor::MatchClock(MatchClock {
                elapsed_seconds: end_match,
            })))
        }
        FactAnchorKind::VideoClock => {
            let Some(end_video) = end_video else {
                return Err(SampleMatchDecodeErrorV2::MissingAnchorBody(
                    "end.videoClock".to_owned(),
                ));
            };
            Ok(Some(FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end_video,
            })))
        }
        FactAnchorKind::Both => {
            let Some(end_match) = end_match else {
                return Err(SampleMatchDecodeErrorV2::MissingAnchorBody(
                    "end.both.matchClock".to_owned(),
                ));
            };
            let Some(end_video) = end_video else {
                return Err(SampleMatchDecodeErrorV2::MissingAnchorBody(
                    "end.both.videoClock".to_owned(),
                ));
            };
            Ok(Some(FactAnchor::Both {
                match_clock: MatchClock {
                    elapsed_seconds: end_match,
                },
                video_clock: VideoClock {
                    elapsed_seconds: end_video,
                },
            }))
        }
    }
}

// ── raw value ──
//
// from_raw は converter（decode）、_raw は exporter（encode）が使う。
// 対で並べ、往復（from_raw(raw(k)) == Some(k)）は exporter テストで固定する。

/// Swift `PlayEventKind(rawValue:)` 相当（rawValue は domain serde の camelCase 表記と一致）。
fn play_event_kind_from_raw(raw: &str) -> Option<PlayEventKind> {
    match raw {
        "goal" => Some(PlayEventKind::Goal),
        "shotMissed" => Some(PlayEventKind::ShotMissed),
        "freeNote" => Some(PlayEventKind::FreeNote),
        "yellowCard" => Some(PlayEventKind::YellowCard),
        "twoMinuteSuspension" => Some(PlayEventKind::TwoMinuteSuspension),
        "redCard" => Some(PlayEventKind::RedCard),
        _ => None,
    }
}

/// Swift `PlayEventKind.rawValue` 相当。
pub(super) fn play_event_kind_raw(kind: PlayEventKind) -> &'static str {
    match kind {
        PlayEventKind::Goal => "goal",
        PlayEventKind::ShotMissed => "shotMissed",
        PlayEventKind::FreeNote => "freeNote",
        PlayEventKind::YellowCard => "yellowCard",
        PlayEventKind::TwoMinuteSuspension => "twoMinuteSuspension",
        PlayEventKind::RedCard => "redCard",
    }
}

/// Swift `PhaseKind(rawValue:)` 相当。
fn phase_kind_from_raw(raw: &str) -> Option<PhaseKind> {
    match raw {
        "regular" => Some(PhaseKind::Regular),
        "shootout" => Some(PhaseKind::Shootout),
        _ => None,
    }
}

/// Swift `PhaseKind.rawValue` 相当。
pub(super) fn phase_kind_raw(kind: PhaseKind) -> &'static str {
    match kind {
        PhaseKind::Regular => "regular",
        PhaseKind::Shootout => "shootout",
    }
}

/// Swift `StoppageKind(rawValue:)` 相当。
fn stoppage_kind_from_raw(raw: &str) -> Option<StoppageKind> {
    match raw {
        "timeout" => Some(StoppageKind::Timeout),
        "pause" => Some(StoppageKind::Pause),
        _ => None,
    }
}

/// Swift `StoppageKind.rawValue` 相当。
pub(super) fn stoppage_kind_raw(kind: StoppageKind) -> &'static str {
    match kind {
        StoppageKind::Timeout => "timeout",
        StoppageKind::Pause => "pause",
    }
}
