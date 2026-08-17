//! ポゼッション開始 fact（handball-project#154）。Rust 新設のためオラクル（Swift）は無い。
//!
//! 語の定義は HandballRecorder の `CONTEXT.md`「ポゼッション開始 (Possession start)」、
//! validation の仕様は `docs/redesign/DOMAIN_VALIDATION_RULES.md`。
//!
//! **「置かないルール」も同じ重みでテストする** — 同一チームの連続 / phase の被覆 /
//! `.videoHighlight` での禁止はいずれも意図的に許しており、後から「バグに見えるから」と
//! 足されると供給源（動画解析）の欠測 1 件で試合まるごと import 拒否になる。

use chrono::{DateTime, Utc};
use handball_toolkit::clock::{FactAnchor, MatchClock, VideoClock};
use handball_toolkit::configuration::{MatchConfiguration, PhaseKind, VideoProvider, VideoSource};
use handball_toolkit::entities::{Match, RosterSelection};
use handball_toolkit::facts::{
    ControlFact, MatchFact, MatchFactPayload, PhaseStartPayload, PossessionFact, StoppageKind,
    StoppagePayload,
};
use handball_toolkit::ids::{FactId, MatchId, TeamId};
use handball_toolkit::sample_dto::{SampleFactDtoV2, SampleMatchDecodeErrorV2, decode_fact};
use handball_toolkit::validation::{
    DomainValidationIssue, FactValidationError, TimelineValidationError,
};
use handball_toolkit::validators::{
    RosterContext, validate_fact_log, validate_match_fact, validate_possession_fact,
};
use std::collections::BTreeMap;
use uuid::Uuid;

fn recorded_at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

struct Ctx {
    home_id: TeamId,
    away_id: TeamId,
    video_match: Match,
    highlight_match: Match,
    timer_config: MatchConfiguration,
    video_config: MatchConfiguration,
}

fn ctx() -> Ctx {
    let home_id = TeamId(Uuid::new_v4());
    let away_id = TeamId(Uuid::new_v4());
    let source = |id: &str| VideoSource {
        provider: VideoProvider::Youtube,
        external_id: id.to_owned(),
    };
    let make_match = |configuration: MatchConfiguration, title: Option<&str>| Match {
        id: MatchId(Uuid::new_v4()),
        title: title.map(str::to_owned),
        date: recorded_at(),
        home_team_id: home_id,
        away_team_id: away_id,
        configuration,
        roster_selection: RosterSelection::default(),
        is_home_on_left: true,
    };
    Ctx {
        home_id,
        away_id,
        video_match: make_match(MatchConfiguration::Video(source("vid")), None),
        highlight_match: make_match(
            MatchConfiguration::VideoHighlight(source("hl")),
            Some("石川空選手の得点。"),
        ),
        timer_config: MatchConfiguration::Timer {
            phase_duration_seconds: 1800.0,
        },
        video_config: MatchConfiguration::Video(source("vid")),
    }
}

fn roster(c: &Ctx) -> RosterContext {
    RosterContext::empty(c.home_id, c.away_id)
}

fn video_possession(team: TeamId, secs: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::new_v4()),
        recorded_at: recorded_at(),
        payload: MatchFactPayload::Possession(PossessionFact {
            team_id: team,
            anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: secs,
            }),
        }),
    }
}

fn video_phase_start(start: f64, end: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::new_v4()),
        recorded_at: recorded_at(),
        payload: MatchFactPayload::Control(ControlFact::PhaseStart(PhaseStartPayload {
            kind: PhaseKind::Regular,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: start,
            }),
            end_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end,
            }),
        })),
    }
}

fn video_stoppage(start: f64, end: f64) -> MatchFact {
    MatchFact {
        id: FactId(Uuid::new_v4()),
        recorded_at: recorded_at(),
        payload: MatchFactPayload::Control(ControlFact::Stoppage(StoppagePayload {
            kind: StoppageKind::Timeout,
            start_anchor: FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: start,
            }),
            end_anchor: Some(FactAnchor::VideoClock(VideoClock {
                elapsed_seconds: end,
            })),
            note: None,
        })),
    }
}

// ── Value / context validation ──

#[test]
fn accepts_video_clock_anchor_in_video_mode() {
    let c = ctx();
    let fact = PossessionFact {
        team_id: c.home_id,
        anchor: FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 754.0,
        }),
    };
    assert!(validate_possession_fact(&fact, &c.video_config, &roster(&c)).is_empty());
}

#[test]
fn rejects_negative_anchor() {
    let c = ctx();
    let fact = PossessionFact {
        team_id: c.home_id,
        anchor: FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: -1.0,
        }),
    };
    let issues = validate_possession_fact(&fact, &c.video_config, &roster(&c));
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::NegativeVideoClock
    )));
}

#[test]
fn rejects_non_finite_anchor() {
    let c = ctx();
    let fact = PossessionFact {
        team_id: c.home_id,
        anchor: FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: f64::NAN,
        }),
    };
    let issues = validate_possession_fact(&fact, &c.video_config, &roster(&c));
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::NonFiniteVideoClock
    )));
}

/// capture method 整合は `FactAnchor` の既存ルールがそのまま乗る
/// （`.timer` は matchClock のみ、`.video` 系は videoClock / both）。
#[test]
fn rejects_video_clock_anchor_in_timer_mode() {
    let c = ctx();
    let fact = PossessionFact {
        team_id: c.home_id,
        anchor: FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 10.0,
        }),
    };
    let issues = validate_possession_fact(&fact, &c.timer_config, &roster(&c));
    assert!(issues.iter().any(|issue| matches!(
        issue,
        DomainValidationIssue::Fact(FactValidationError::InvalidAnchorForConfiguration { .. })
    )));
}

#[test]
fn accepts_match_clock_anchor_in_timer_mode() {
    let c = ctx();
    let fact = PossessionFact {
        team_id: c.home_id,
        anchor: FactAnchor::MatchClock(MatchClock {
            elapsed_seconds: 10.0,
        }),
    };
    assert!(validate_possession_fact(&fact, &c.timer_config, &roster(&c)).is_empty());
}

#[test]
fn rejects_team_outside_the_match() {
    let c = ctx();
    let stranger = TeamId(Uuid::new_v4());
    let fact = PossessionFact {
        team_id: stranger,
        anchor: FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 100.0,
        }),
    };
    let issues = validate_possession_fact(&fact, &c.video_config, &roster(&c));
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::UnknownTeamReference { team_id: stranger }
    )));
}

/// `validate_match_fact` の dispatch が第 3 variant を拾うこと（分岐漏れの回帰防止）。
#[test]
fn match_fact_dispatch_reaches_possession() {
    let c = ctx();
    let stranger = TeamId(Uuid::new_v4());
    let fact = video_possession(stranger, 100.0);
    let issues = validate_match_fact(&fact, &c.video_config, &roster(&c));
    assert!(issues.contains(&DomainValidationIssue::Fact(
        FactValidationError::UnknownTeamReference { team_id: stranger }
    )));
}

// ── R7 / R8（「anchor を 1 本持つ fact」へ一般化した分） ──

#[test]
fn r7_possession_outside_phase_range_is_blocking() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        // ハーフタイム中に保持が移った = 供給源の典型的な故障。
        video_possession(c.home_id, 1900.0),
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(issues.iter().any(|issue| matches!(
        issue,
        DomainValidationIssue::Timeline(
            TimelineValidationError::PlayRecordedOutsidePhaseRange { .. }
        )
    )));
}

#[test]
fn r7_possession_inside_phase_range_is_accepted() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_possession(c.home_id, 600.0),
    ];
    assert!(validate_fact_log(&facts, &c.video_match).is_empty());
}

#[test]
fn r8_possession_inside_stoppage_is_blocking() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_stoppage(500.0, 600.0),
        // タイムアウト中に保持は移らない（ハンドボールでは起こらない）。
        video_possession(c.away_id, 550.0),
    ];
    let issues = validate_fact_log(&facts, &c.video_match);
    assert!(issues.contains(&DomainValidationIssue::Timeline(
        TimelineValidationError::PlayRecordedInsideStoppage
    )));
}

// ── 意図的に「置かない」ルール（DOMAIN_VALIDATION_RULES.md「持たないルール」） ──

/// 同一チームの連続は矛盾ではなく 2 件目が冗長なだけ。禁止すると供給源の欠測 1 件で
/// 試合まるごと import 拒否になる（severity は一律 blocking で warning が無いため）。
#[test]
fn consecutive_possessions_by_the_same_team_are_accepted() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_possession(c.home_id, 600.0),
        video_possession(c.home_id, 640.0),
    ];
    assert!(validate_fact_log(&facts, &c.video_match).is_empty());
}

/// phase を隙間なく覆っている必要は無い（カバレッジ欠損は正常な入力）。
#[test]
fn sparse_possession_coverage_is_accepted() {
    let c = ctx();
    let facts = vec![
        video_phase_start(0.0, 1800.0),
        video_possession(c.home_id, 1700.0),
    ];
    assert!(validate_fact_log(&facts, &c.video_match).is_empty());
}

/// `.videoHighlight` で禁止しない（R6 / R9 と違い構造的な矛盾が無い）。
/// R7 / R8 は phase / Stoppage が 0 件なので早期 return して自然に無効化される。
#[test]
fn possession_in_video_highlight_is_accepted() {
    let c = ctx();
    let facts = vec![video_possession(c.home_id, 12.0)];
    assert!(validate_fact_log(&facts, &c.highlight_match).is_empty());
}

// ── DTO decode ──

fn teams_by_key(c: &Ctx) -> BTreeMap<String, TeamId> {
    BTreeMap::from([
        ("home".to_owned(), c.home_id),
        ("away".to_owned(), c.away_id),
    ])
}

fn decode(c: &Ctx, json: &str) -> Result<MatchFact, SampleMatchDecodeErrorV2> {
    let dto: SampleFactDtoV2 = serde_json::from_str(json).unwrap();
    decode_fact(&dto, &teams_by_key(c), &BTreeMap::new(), Uuid::new_v4)
}

#[test]
fn decodes_possession_payload() {
    let c = ctx();
    let json = r#"{
        "recordedAt": "2025-04-15T13:01:37Z",
        "payload": {
            "kind": "possession",
            "possession": {
                "teamKey": "away",
                "anchor": { "kind": "videoClock", "videoClock": { "elapsedSeconds": 97 } }
            }
        }
    }"#;
    let fact = decode(&c, json).unwrap();
    let MatchFactPayload::Possession(possession) = fact.payload else {
        panic!("possession payload を期待");
    };
    assert_eq!(possession.team_id, c.away_id);
    assert_eq!(
        possession.anchor,
        FactAnchor::VideoClock(VideoClock {
            elapsed_seconds: 97.0
        })
    );
}

#[test]
fn rejects_possession_payload_without_body() {
    let c = ctx();
    let json = r#"{"recordedAt": "2025-04-15T13:01:37Z", "payload": {"kind": "possession"}}"#;
    assert!(matches!(
        decode(&c, json),
        Err(SampleMatchDecodeErrorV2::MissingPayloadBody(kind)) if kind == "possession"
    ));
}

#[test]
fn rejects_unknown_team_key() {
    let c = ctx();
    let json = r#"{
        "recordedAt": "2025-04-15T13:01:37Z",
        "payload": {
            "kind": "possession",
            "possession": {
                "teamKey": "neutral",
                "anchor": { "kind": "videoClock", "videoClock": { "elapsedSeconds": 97 } }
            }
        }
    }"#;
    assert!(matches!(
        decode(&c, json),
        Err(SampleMatchDecodeErrorV2::UnknownTeamKey(key)) if key == "neutral"
    ));
}

/// `possession` フィールドを持たない既存 JSON がそのまま読めること（後方互換）。
#[test]
fn existing_play_payload_still_decodes() {
    let c = ctx();
    let json = r#"{
        "recordedAt": "2025-04-15T13:01:35Z",
        "payload": {
            "kind": "play",
            "play": {
                "kind": "goal",
                "teamKey": "home",
                "anchor": { "kind": "videoClock", "videoClock": { "elapsedSeconds": 95.5 } }
            }
        }
    }"#;
    let fact = decode(&c, json).unwrap();
    assert!(matches!(fact.payload, MatchFactPayload::Play(_)));
}
