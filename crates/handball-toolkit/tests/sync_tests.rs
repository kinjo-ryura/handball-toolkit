//! 端末どうしの同期（`sync`。handball-project#506 / ADR 0007）。
//!
//! 固定する挙動:
//! - 記録ごとに `updated_at` の新しい方を採る。片方にしか無いものは足す
//! - 規則で決められないもの（同じ時刻 / 消した後の変更 / 混ざった試合が規則に合わない / 写しの中身が違う）だけを
//!   問いにし、答えた版を `now` で書き直す
//! - ID の違う写し（試合ファイルで受け取ったもの）は、印の少ない側の半分以上が一致するときだけ
//!   1 つにする。同じ試合を 2 台で別々に記録したものはまとめない
//! - 端末ごとの値（左右配置・写真・端末内動画の参照）は比べず、保存する端末の値を残す
//! - 時刻はミリ秒に丸めて比べる（端末との往復で ns の桁がずれても問いを出さない）
//! - 運ぶ形は版を先に見る。f64 も時刻もそのまま往復する

mod fixtures;

use chrono::{DateTime, TimeDelta, Utc};
use handball_toolkit::configuration::{MatchConfiguration, VideoProvider, VideoSource};
use handball_toolkit::entities::{Match, Player, PlayerPhoto, Team};
use handball_toolkit::facts::{MatchFact, MatchFactPayload, PlayEventKind};
use handball_toolkit::ids::{FactId, MatchId, PlayerId, TeamId};
use handball_toolkit::sync::{
    LocalVideoIdentity, SYNC_FORMAT_VERSION, SyncAnswer, SyncFact, SyncMatch, SyncPayload,
    SyncPayloadError, SyncPlayer, SyncQuestion, SyncQuestionKind, SyncReconcileResult,
    SyncRecordRef, SyncSide, SyncSnapshot, SyncStamp, SyncTeam, decode_sync_payload,
    encode_sync_payload, materialize, reconcile,
};
use uuid::Uuid;

use fixtures::{epoch, make_timer_match, play_at_match, timer_phase};

// ── 記録ごとの後勝ち ──

#[test]
fn identical_devices_merge_to_the_same_records() {
    let w = World::new();
    let local = w.snapshot(alive(1));
    let remote = w.snapshot(alive(1));

    let merged = merged(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(merged, local);
}

#[test]
fn records_only_on_one_side_are_added() {
    let w = World::new();
    let local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    let extra = goal(FactId(Uuid::from_u128(30)), &w, 900.0);
    remote.facts.push(w.fact(&extra, alive(5)));

    let merged = merged(reconcile(&local, &remote, &[], at(100)));

    assert!(merged.facts.iter().any(|f| f.fact.id == extra.id));
    assert_eq!(merged.facts.len(), 3);
}

#[test]
fn newer_updated_at_wins_without_asking() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    set_title(&mut local, "1試合目", alive(10));
    set_title(&mut remote, "決勝", alive(20));

    let merged = merged(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(merged.matches[0].match_.title.as_deref(), Some("決勝"));
    assert_eq!(merged.matches[0].stamp, alive(20));
}

/// 試合と fact は別々に比べる。片方でタイトルを直し、もう片方で fact を足しても両方残る。
#[test]
fn match_and_fact_changes_from_different_devices_both_survive() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    set_title(&mut local, "決勝", alive(10));
    let extra = goal(FactId(Uuid::from_u128(30)), &w, 900.0);
    remote.facts.push(w.fact(&extra, alive(20)));

    let merged = merged(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(merged.matches[0].match_.title.as_deref(), Some("決勝"));
    assert!(merged.facts.iter().any(|f| f.fact.id == extra.id));
}

// ── 聞くこと ──

#[test]
fn same_time_with_different_content_is_asked_and_the_answer_is_restamped() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    set_title(&mut local, "1試合目", alive(10));
    set_title(&mut remote, "決勝", alive(10));
    let record = SyncRecordRef::Match { id: w.match_.id };

    let asked = questions(reconcile(&local, &remote, &[], at(100)));
    assert_eq!(
        asked,
        vec![SyncQuestion {
            kind: SyncQuestionKind::SameTime,
            record,
            match_id: Some(w.match_.id),
            deleted_on: None,
        }]
    );

    let answer = SyncAnswer {
        kind: SyncQuestionKind::SameTime,
        record,
        keep: SyncSide::Local,
    };
    let merged = merged(reconcile(&local, &remote, &[answer], at(100)));
    assert_eq!(merged.matches[0].match_.title.as_deref(), Some("1試合目"));
    assert_eq!(merged.matches[0].stamp, alive(100));
}

#[test]
fn deletion_after_the_last_change_wins_without_asking() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    set_fact_stamp(&mut local, w.goal.id, deleted(30));
    set_fact_note(&mut remote, w.goal.id, "速攻", alive(20));

    let merged = merged(reconcile(&local, &remote, &[], at(100)));

    let goal = merged
        .facts
        .iter()
        .find(|f| f.fact.id == w.goal.id)
        .unwrap();
    assert_eq!(goal.stamp, deleted(30));
}

#[test]
fn deletion_before_a_change_on_the_other_device_is_asked() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    set_fact_stamp(&mut local, w.goal.id, deleted(20));
    set_fact_note(&mut remote, w.goal.id, "速攻", alive(30));
    let record = SyncRecordRef::Fact { id: w.goal.id };

    let asked = questions(reconcile(&local, &remote, &[], at(100)));
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].kind, SyncQuestionKind::DeletedThenChanged);
    assert_eq!(asked[0].record, record);
    assert_eq!(asked[0].match_id, Some(w.match_.id));
    assert_eq!(asked[0].deleted_on, Some(SyncSide::Local));

    // 消した側を選ぶと、今の時刻で消し直す（後で古い版を持つ端末と比べても消える）。
    let delete = SyncAnswer {
        kind: SyncQuestionKind::DeletedThenChanged,
        record,
        keep: SyncSide::Local,
    };
    let merged_delete = merged(reconcile(&local, &remote, &[delete], at(100)));
    let goal = merged_delete
        .facts
        .iter()
        .find(|f| f.fact.id == w.goal.id)
        .unwrap();
    assert_eq!(goal.stamp, deleted(100));

    // 変えた側を選ぶと、変えた版を今の時刻で残す。
    let keep = SyncAnswer {
        kind: SyncQuestionKind::DeletedThenChanged,
        record,
        keep: SyncSide::Remote,
    };
    let merged_keep = merged(reconcile(&local, &remote, &[keep], at(100)));
    let goal = merged_keep
        .facts
        .iter()
        .find(|f| f.fact.id == w.goal.id)
        .unwrap();
    assert_eq!(goal.stamp, alive(100));
    assert_eq!(note_of(&goal.fact), Some("速攻"));
}

/// 試合の削除は、相手での fact の変更とも比べる（試合の `updated_at` は fact の変更で進まない）。
#[test]
fn match_deletion_is_compared_with_fact_changes_on_the_other_device() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    local.matches[0].stamp = deleted(20);
    set_fact_note(&mut remote, w.goal.id, "速攻", alive(30));

    let asked = questions(reconcile(&local, &remote, &[], at(100)));

    assert!(asked.iter().any(|q| {
        q.kind == SyncQuestionKind::DeletedThenChanged
            && q.record == SyncRecordRef::Match { id: w.match_.id }
            && q.deleted_on == Some(SyncSide::Local)
    }));
}

/// 両方の直しを合わせると規則に合わない試合は聞き、答えた側の試合を丸ごと採る。
///
/// この端末は前半を 0〜1800 秒に縮めて後半（1800〜3600 秒）を足し、相手は前半を 0〜2400 秒に
/// 延ばした（どちらもそれだけなら規則に合う）。混ぜると前半の終わり（2400）と後半の始まり（1800）が
/// 食い違い、タイマーの区切りの連続性に反する。
#[test]
fn merged_match_that_breaks_the_rules_is_asked_and_one_side_is_kept_whole() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    let second_half = timer_phase(FactId(Uuid::from_u128(31)), 1800.0, 3600.0);
    replace_fact(&mut local, timer_phase(w.phase.id, 0.0, 1800.0), alive(30));
    local.facts.push(w.fact(&second_half, alive(30)));
    replace_fact(&mut remote, timer_phase(w.phase.id, 0.0, 2400.0), alive(40));
    let record = SyncRecordRef::Match { id: w.match_.id };

    let asked = questions(reconcile(&local, &remote, &[], at(100)));
    assert_eq!(
        asked,
        vec![SyncQuestion {
            kind: SyncQuestionKind::MergedMatchInvalid,
            record,
            match_id: Some(w.match_.id),
            deleted_on: None,
        }]
    );

    let answer = SyncAnswer {
        kind: SyncQuestionKind::MergedMatchInvalid,
        record,
        keep: SyncSide::Local,
    };
    let merged = merged(reconcile(&local, &remote, &[answer], at(100)));
    let first_half = merged
        .facts
        .iter()
        .find(|f| f.fact.id == w.phase.id)
        .unwrap();
    assert_eq!(first_half.fact, timer_phase(w.phase.id, 0.0, 1800.0));
    assert_eq!(first_half.stamp, alive(100));
    let kept_second_half = merged
        .facts
        .iter()
        .find(|f| f.fact.id == second_half.id)
        .unwrap();
    assert_eq!(kept_second_half.stamp, alive(100));
}

/// 片方の版そのままの試合は検証しない（その端末で保存するときに検証を通っている）。
/// 混ざっていない試合で問いが出ないことを、上の筋書きの片側だけで確かめる。
#[test]
fn match_taken_whole_from_one_side_is_not_revalidated() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let remote = w.snapshot(alive(1));
    let second_half = timer_phase(FactId(Uuid::from_u128(31)), 1800.0, 3600.0);
    replace_fact(&mut local, timer_phase(w.phase.id, 0.0, 1800.0), alive(30));
    local.facts.push(w.fact(&second_half, alive(30)));

    let result = reconcile(&local, &remote, &[], at(100));

    assert!(matches!(result, SyncReconcileResult::Merged { .. }));
}

/// 片方で消した試合を「残す」と答えたら、両方の直しを合わせて規則に合わなくても、もう聞かずに
/// 生きている側の試合を丸ごと採る（handball-project#510 — 続けて聞いた問いで消した側を選ぶと、
/// 残すと答えた試合が消えていた）。
///
/// この端末は前半を縮めて後半を足してから試合を消し、相手は消される前の試合で前半を延ばした。
#[test]
fn match_kept_after_deletion_is_taken_whole_from_the_alive_side() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    let second_half = timer_phase(FactId(Uuid::from_u128(31)), 1800.0, 3600.0);
    replace_fact(&mut local, timer_phase(w.phase.id, 0.0, 1800.0), alive(10));
    local.facts.push(w.fact(&second_half, alive(10)));
    local.matches[0].stamp = deleted(20);
    replace_fact(&mut remote, timer_phase(w.phase.id, 0.0, 2400.0), alive(30));
    let record = SyncRecordRef::Match { id: w.match_.id };

    let asked = questions(reconcile(&local, &remote, &[], at(100)));
    assert_eq!(
        asked,
        vec![SyncQuestion {
            kind: SyncQuestionKind::DeletedThenChanged,
            record,
            match_id: Some(w.match_.id),
            deleted_on: Some(SyncSide::Local),
        }]
    );

    let keep = SyncAnswer {
        kind: SyncQuestionKind::DeletedThenChanged,
        record,
        keep: SyncSide::Remote,
    };
    // 前に出ていた問いへの答え（消した側）が残っていても使わない。
    let stale = SyncAnswer {
        kind: SyncQuestionKind::MergedMatchInvalid,
        record,
        keep: SyncSide::Local,
    };
    let assert_alive_side_taken = |answers: &[SyncAnswer]| {
        let merged = merged(reconcile(&local, &remote, answers, at(100)));
        let kept = merged
            .matches
            .iter()
            .find(|m| m.match_.id == w.match_.id)
            .unwrap();
        assert_eq!(kept.stamp, alive(100));
        let first_half = merged
            .facts
            .iter()
            .find(|f| f.fact.id == w.phase.id)
            .unwrap();
        assert_eq!(first_half.fact, timer_phase(w.phase.id, 0.0, 2400.0));
        assert_eq!(first_half.stamp, alive(100));
        let dropped = merged
            .facts
            .iter()
            .find(|f| f.fact.id == second_half.id)
            .unwrap();
        assert_eq!(dropped.stamp, deleted(100));
    };
    assert_alive_side_taken(&[keep]);
    assert_alive_side_taken(&[keep, stale]);
}

// ── 参照の整合 ──

/// 片方で（使われていなかった）チームを消し、もう片方でそのチームの試合を足していたら、チームを戻す。
#[test]
fn team_referenced_by_a_live_match_is_revived() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    let spare = Team {
        id: TeamId(Uuid::from_u128(4)),
        name: "Spare".to_owned(),
    };
    local.teams.push(SyncTeam {
        team: spare.clone(),
        stamp: deleted(30),
    });
    remote.teams.push(SyncTeam {
        team: spare.clone(),
        stamp: alive(1),
    });
    let mut other = make_timer_match(w.home.id, spare.id);
    other.id = MatchId(Uuid::from_u128(11));
    remote.matches.push(SyncMatch {
        match_: other,
        stamp: alive(20),
        local_video: None,
    });

    let merged = merged(reconcile(&local, &remote, &[], at(100)));

    let revived = merged.teams.iter().find(|t| t.team.id == spare.id).unwrap();
    assert_eq!(revived.stamp, alive(100));
}

// ── 時刻の精度 ──

/// 端末との往復で ns の桁がずれても、同期済みの同じ記録を「同じ時刻で中身が違う」と見ない。
#[test]
fn nanosecond_drift_from_the_round_trip_is_not_a_difference() {
    let w = World::new();
    let local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    for f in &mut remote.facts {
        f.fact.recorded_at += TimeDelta::nanoseconds(120);
        f.stamp.updated_at -= TimeDelta::nanoseconds(90);
    }

    let result = reconcile(&local, &remote, &[], at(100));

    assert_eq!(merged(result), local);
}

// ── 端末ごとの値 ──

#[test]
fn materialize_keeps_this_devices_view_settings_and_photos() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    local.matches[0].match_.is_home_on_left = false;
    local.players[0].player.photo = Some(PlayerPhoto {
        storage_key: "alice.jpg".to_owned(),
    });
    let mut incoming = w.snapshot(alive(1));
    incoming.players[0].player.photo = Some(PlayerPhoto {
        storage_key: "other-device.jpg".to_owned(),
    });
    let newcomer = Player {
        id: PlayerId(Uuid::from_u128(5)),
        team_id: w.home.id,
        name: "Bob".to_owned(),
        jersey_number: Some(9),
        photo: Some(PlayerPhoto {
            storage_key: "bob-other-device.jpg".to_owned(),
        }),
    };
    incoming.players.push(SyncPlayer {
        player: newcomer.clone(),
        stamp: alive(5),
    });

    let plan = materialize(&incoming, &local);

    assert!(!plan.snapshot.matches[0].match_.is_home_on_left);
    let alice = plan
        .snapshot
        .players
        .iter()
        .find(|p| p.player.id == w.player.id)
        .unwrap();
    assert_eq!(
        alice.player.photo.as_ref().map(|p| p.storage_key.as_str()),
        Some("alice.jpg")
    );
    let bob = plan
        .snapshot
        .players
        .iter()
        .find(|p| p.player.id == newcomer.id)
        .unwrap();
    assert_eq!(bob.player.photo, None);
    assert!(plan.video_relinks.is_empty());
}

#[test]
fn materialize_keeps_this_devices_reference_to_the_same_local_video() {
    let w = World::new();
    let local = with_local_video(w.snapshot(alive(1)), "iphone-asset", Some("cloud-1"));
    let incoming = with_local_video(w.snapshot(alive(1)), "ipad-asset", Some("cloud-1"));

    let plan = materialize(&incoming, &local);

    assert_eq!(
        external_id(&plan.snapshot.matches[0].match_),
        "iphone-asset"
    );
    assert!(plan.video_relinks.is_empty());
}

#[test]
fn materialize_asks_to_relink_a_different_local_video() {
    let w = World::new();
    let local = with_local_video(w.snapshot(alive(1)), "iphone-asset", Some("cloud-1"));
    let incoming = with_local_video(w.snapshot(alive(1)), "ipad-asset", Some("cloud-2"));

    let plan = materialize(&incoming, &local);

    assert_eq!(external_id(&plan.snapshot.matches[0].match_), "ipad-asset");
    assert_eq!(plan.video_relinks.len(), 1);
    assert_eq!(plan.video_relinks[0].match_id, w.match_.id);
    assert_eq!(
        plan.video_relinks[0].identity.cloud_identifier.as_deref(),
        Some("cloud-2")
    );
}

/// 端末内動画の参照（localIdentifier）が違うだけなら、同じ中身として扱う。
#[test]
fn local_video_references_alone_are_not_a_difference() {
    let w = World::new();
    let local = with_local_video(w.snapshot(alive(1)), "iphone-asset", Some("cloud-1"));
    let mut remote = with_local_video(w.snapshot(alive(1)), "ipad-asset", Some("cloud-1"));
    remote.matches[0].match_.is_home_on_left = false;

    let result = reconcile(&local, &remote, &[], at(100));

    assert!(matches!(result, SyncReconcileResult::Merged { .. }));
}

// ── 突き合わせ ──

/// 試合ファイルで受け取った写し（ID が全部違う）が元の試合と同じ中身なら、聞かずに 1 つにする。
/// 小さい方の ID を残し、写しの試合・fact・名前の同じチームと選手を消す。
#[test]
fn identical_copy_from_a_match_file_is_merged_without_asking() {
    let w = World::new();
    let local = w.snapshot(alive(1));
    let remote = MatchFileCopy::new(&w).snapshot(alive(2));

    let result = reconcile(&local, &remote, &[], at(100));

    let (merged, duplicates) = merged_with_duplicates(result);
    assert!(duplicates.is_empty());
    let live_matches: Vec<MatchId> = merged
        .matches
        .iter()
        .filter(|m| m.stamp.deleted_at.is_none())
        .map(|m| m.match_.id)
        .collect();
    assert_eq!(live_matches, vec![w.match_.id]);
    let copy = MatchFileCopy::new(&w);
    for team in [copy.home, copy.away] {
        let t = merged.teams.iter().find(|t| t.team.id == team).unwrap();
        assert_eq!(t.stamp, deleted(100));
    }
    let p = merged
        .players
        .iter()
        .find(|p| p.player.id == copy.player)
        .unwrap();
    assert_eq!(p.stamp, deleted(100));
    for f in merged.facts.iter().filter(|f| f.match_id == copy.match_id) {
        assert_eq!(f.stamp, deleted(100));
    }
}

/// 写しの中身が違えば、どちらを残すかを聞く。答えた側の ID と中身を残し、もう片方を消す。
/// 名前の同じチームと選手は、残した側へまとめる。
#[test]
fn copy_with_different_content_is_asked_and_the_answer_keeps_that_side() {
    let w = World::new();
    let local = w.snapshot(alive(1));
    let copy = MatchFileCopy::new(&w);
    let mut remote = copy.snapshot(alive(2));
    set_fact_note(&mut remote, copy.goal, "速攻", alive(3));
    let record = SyncRecordRef::Match { id: w.match_.id };

    let asked = questions(reconcile(&local, &remote, &[], at(100)));
    assert_eq!(
        asked,
        vec![SyncQuestion {
            kind: SyncQuestionKind::CopiesDiffer,
            record,
            match_id: Some(w.match_.id),
            deleted_on: None,
        }]
    );

    let answer = SyncAnswer {
        kind: SyncQuestionKind::CopiesDiffer,
        record,
        keep: SyncSide::Remote,
    };
    let merged = merged(reconcile(&local, &remote, &[answer], at(100)));
    let kept = merged
        .matches
        .iter()
        .find(|m| m.match_.id == copy.match_id)
        .unwrap();
    assert_eq!(kept.stamp, alive(100));
    let original = merged
        .matches
        .iter()
        .find(|m| m.match_.id == w.match_.id)
        .unwrap();
    assert_eq!(original.stamp, deleted(100));
    let home = merged
        .teams
        .iter()
        .find(|t| t.team.id == w.home.id)
        .unwrap();
    assert_eq!(home.stamp, deleted(100));
    let alice = merged
        .players
        .iter()
        .find(|p| p.player.id == w.player.id)
        .unwrap();
    assert_eq!(alice.stamp, deleted(100));
}

/// 同じ端末の中の写しどうしはまとめず、同じ試合に見える組として知らせる。
#[test]
fn copies_on_the_same_device_are_left_and_reported() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    let copy = MatchFileCopy::new(&w);
    let copied = copy.snapshot(alive(2));
    local.matches.extend(copied.matches);
    local.teams.extend(copied.teams);
    local.players.extend(copied.players);
    local.facts.extend(copied.facts);
    let remote = SyncSnapshot::default();

    let (merged, duplicates) = merged_with_duplicates(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(merged.matches.len(), 2);
    assert!(merged.matches.iter().all(|m| m.stamp.deleted_at.is_none()));
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].match_ids, vec![w.match_.id, copy.match_id]);
}

/// 同期済みの試合（両方の端末で同じ ID）に、後から試合ファイルで写しが届いていても、まとめずに
/// 別の試合として足し、同じ試合に見える組として知らせる。
#[test]
fn copy_of_an_already_synced_match_is_added_and_reported() {
    let w = World::new();
    let local = w.snapshot(alive(1));
    let mut remote = w.snapshot(alive(1));
    let copy = MatchFileCopy::new(&w);
    let copied = copy.snapshot(alive(2));
    remote.matches.extend(copied.matches);
    remote.teams.extend(copied.teams);
    remote.players.extend(copied.players);
    remote.facts.extend(copied.facts);

    let (merged, duplicates) = merged_with_duplicates(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(
        merged
            .matches
            .iter()
            .filter(|m| m.stamp.deleted_at.is_none())
            .count(),
        2
    );
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].match_ids, vec![w.match_.id, copy.match_id]);
}

/// 試合ファイルで受け取った写しは、時刻が秒ちょうどになる（ファイルは秒までしか書かない）。元の記録が
/// ミリ秒まで持っていても、同じ秒の印として一致する。
#[test]
fn match_file_copy_with_whole_second_times_is_still_a_copy() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    set_recorded_at(&mut local, w.phase.id, at_ms(5_347));
    set_recorded_at(&mut local, w.goal.id, at_ms(700_120));
    let copy = MatchFileCopy::new(&w);
    let mut remote = copy.snapshot(alive(2));
    set_recorded_at(&mut remote, copy.phase, at(5));
    set_recorded_at(&mut remote, copy.goal, at(700));

    let (merged, duplicates) = merged_with_duplicates(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(live_match_ids(&merged), vec![w.match_.id]);
    assert!(duplicates.is_empty());
}

/// 同じ試合を 2 台で別々に記録したものは写しとみなさない。前半開始を同じ秒に押していても、端末で
/// 記録した時刻はミリ秒まで持つので食い違う（handball-project#510 — 写しとみなして聞いた答えで、
/// 片方の記録が丸ごと消えていた）。相手の記録は ID の振り方だけ写しと同じ（全部違う）。
#[test]
fn separate_recordings_of_the_same_match_are_not_copies() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    set_recorded_at(&mut local, w.phase.id, at_ms(5_347));
    set_recorded_at(&mut local, w.goal.id, at_ms(700_120));
    let other = MatchFileCopy::new(&w);
    let mut remote = other.snapshot(alive(2));
    set_recorded_at(&mut remote, other.phase, at_ms(5_812));
    set_recorded_at(&mut remote, other.goal, at_ms(703_400));

    let (merged, duplicates) = merged_with_duplicates(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(live_match_ids(&merged), vec![w.match_.id, other.match_id]);
    assert!(duplicates.is_empty());
}

/// 別の人の記録の試合ファイル（秒ちょうど）が手元の記録と同じ秒に偶然当たっても、一致する印が
/// 少ない側の半分に届かなければ写しとみなさない。
#[test]
fn match_file_sharing_less_than_half_of_the_prints_is_not_a_copy() {
    let w = World::new();
    let mut local = w.snapshot(alive(1));
    set_recorded_at(&mut local, w.phase.id, at_ms(5_347));
    set_recorded_at(&mut local, w.goal.id, at_ms(700_120));
    let mut extra = goal(FactId(Uuid::from_u128(30)), &w, 900.0);
    extra.recorded_at = at_ms(800_500);
    local.facts.push(w.fact(&extra, alive(1)));
    let other = MatchFileCopy::new(&w);
    let mut remote = other.snapshot(alive(2));
    // 前半開始だけが手元と同じ秒（3 つのうち 1 つ）。
    set_recorded_at(&mut remote, other.phase, at(5));
    set_recorded_at(&mut remote, other.goal, at(650));
    let mut other_goal = play_at_match(PlayEventKind::Goal, other.home, other.player, 1200.0);
    other_goal.id = FactId(Uuid::from_u128(62));
    other_goal.recorded_at = at(1000);
    remote.facts.push(SyncFact {
        match_id: other.match_id,
        fact: other_goal,
        stamp: alive(2),
    });

    let (merged, duplicates) = merged_with_duplicates(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(live_match_ids(&merged), vec![w.match_.id, other.match_id]);
    assert!(duplicates.is_empty());
}

/// 2 台が互いに試合ファイルを送り合っていると、組ごとに残す側が逆になる（この端末の試合 1 と、相手の
/// 試合 2 が残る）。チームはどちらの組でも同じ行き先へまとめ、同じ名前のチームと選手を 1 つずつ残す
/// （handball-project#510 — 向きのある票のまま互いに付け替え合い、参照のある両方が戻っていた）。
#[test]
fn copies_exchanged_both_ways_leave_one_team_and_player_per_name() {
    let w = World::new();
    let first = w.match_.id;
    let second = MatchId(Uuid::from_u128(40));
    let (b_home, b_away) = (TeamId(Uuid::from_u128(101)), TeamId(Uuid::from_u128(102)));
    let b_alice = PlayerId(Uuid::from_u128(103));

    // この端末: 自分で記録した試合 1 と、相手から試合ファイルで受け取った試合 2 の写し（ID 60。
    // 取り込みで自分の同じ名前のチームに紐付けた）。
    let mut local = w.snapshot(alive(1));
    let (copy_of_second, copy_facts) = timer_match_records(
        MatchId(Uuid::from_u128(60)),
        (w.home.id, w.away.id),
        w.player.id,
        [(70, at(500)), (71, at(700))],
        alive(2),
    );
    local.matches.push(copy_of_second);
    local.facts.extend(copy_facts);

    // 相手の端末: 自分のチームと選手、自分で記録した試合 2 と、受け取った試合 1 の写し（ID 150）。
    let (original_second, second_facts) = timer_match_records(
        second,
        (b_home, b_away),
        b_alice,
        [(80, at_ms(500_250)), (81, at_ms(700_125))],
        alive(1),
    );
    let (copy_of_first, first_copy_facts) = timer_match_records(
        MatchId(Uuid::from_u128(150)),
        (b_home, b_away),
        b_alice,
        [(160, epoch()), (161, epoch())],
        alive(2),
    );
    let remote = SyncSnapshot {
        matches: vec![original_second, copy_of_first],
        teams: vec![
            SyncTeam {
                team: Team {
                    id: b_home,
                    name: w.home.name.clone(),
                },
                stamp: alive(1),
            },
            SyncTeam {
                team: Team {
                    id: b_away,
                    name: w.away.name.clone(),
                },
                stamp: alive(1),
            },
        ],
        players: vec![SyncPlayer {
            player: Player {
                id: b_alice,
                team_id: b_home,
                ..w.player.clone()
            },
            stamp: alive(1),
        }],
        facts: second_facts.into_iter().chain(first_copy_facts).collect(),
    };

    let merged = merged(reconcile(&local, &remote, &[], at(100)));

    assert_eq!(live_match_ids(&merged), vec![first, second]);
    let mut team_names: Vec<&str> = merged
        .teams
        .iter()
        .filter(|t| !t.stamp.is_deleted())
        .map(|t| t.team.name.as_str())
        .collect();
    team_names.sort_unstable();
    assert_eq!(team_names, vec!["Away", "Home"]);
    let player_names: Vec<&str> = merged
        .players
        .iter()
        .filter(|p| !p.stamp.is_deleted())
        .map(|p| p.player.name.as_str())
        .collect();
    assert_eq!(player_names, vec!["Alice"]);
    // 残った試合は、残ったチームを指す。
    let live_team = |id: TeamId| {
        merged
            .teams
            .iter()
            .any(|t| t.team.id == id && !t.stamp.is_deleted())
    };
    for m in merged.matches.iter().filter(|m| !m.stamp.is_deleted()) {
        assert!(live_team(m.match_.home_team_id) && live_team(m.match_.away_team_id));
    }
}

// ── 運ぶ形 ──

#[test]
fn payload_round_trips_exactly() {
    let w = World::new();
    let mut snapshot = w.snapshot(alive(1));
    // f64 と ns の時刻が、JSON を往復しても 1 bit も変わらない。
    replace_fact(&mut snapshot, goal(w.goal.id, &w, 0.1 + 0.2), alive(1));
    snapshot.facts[0].stamp.updated_at += TimeDelta::nanoseconds(123_456_789);
    let payload = SyncPayload {
        format_version: SYNC_FORMAT_VERSION,
        device_id: Uuid::from_u128(99),
        snapshot,
    };

    let json = encode_sync_payload(&payload).unwrap();

    assert_eq!(decode_sync_payload(&json).unwrap(), payload);
}

#[test]
fn payload_with_another_format_version_is_rejected_before_reading_it() {
    let json = format!(
        r#"{{"formatVersion": {}, "somethingNew": true}}"#,
        SYNC_FORMAT_VERSION + 1
    );

    assert_eq!(
        decode_sync_payload(&json),
        Err(SyncPayloadError::UnsupportedFormatVersion {
            found: SYNC_FORMAT_VERSION + 1,
            supported: SYNC_FORMAT_VERSION,
        })
    );
}

#[test]
fn payload_that_is_not_json_is_invalid() {
    assert!(matches!(
        decode_sync_payload("not json"),
        Err(SyncPayloadError::InvalidJson { .. })
    ));
}

// ── helpers ──

/// 両方の端末に共通の記録: チーム 2・選手 1・タイマーの試合 1（区切り 0〜3600 秒・得点 1）。
#[derive(Clone)]
struct World {
    home: Team,
    away: Team,
    player: Player,
    match_: Match,
    phase: MatchFact,
    goal: MatchFact,
}

impl World {
    fn new() -> World {
        let home = Team {
            id: TeamId(Uuid::from_u128(1)),
            name: "Home".to_owned(),
        };
        let away = Team {
            id: TeamId(Uuid::from_u128(2)),
            name: "Away".to_owned(),
        };
        let player = Player {
            id: PlayerId(Uuid::from_u128(3)),
            team_id: home.id,
            name: "Alice".to_owned(),
            jersey_number: Some(7),
            photo: None,
        };
        let mut match_ = make_timer_match(home.id, away.id);
        match_.id = MatchId(Uuid::from_u128(10));
        let phase = timer_phase(FactId(Uuid::from_u128(20)), 0.0, 3600.0);
        let mut w = World {
            home,
            away,
            player,
            match_,
            phase,
            goal: timer_phase(FactId(Uuid::from_u128(0)), 0.0, 0.0),
        };
        w.goal = goal(FactId(Uuid::from_u128(21)), &w, 600.0);
        w
    }

    fn snapshot(&self, stamp: SyncStamp) -> SyncSnapshot {
        SyncSnapshot {
            matches: vec![SyncMatch {
                match_: self.match_.clone(),
                stamp,
                local_video: None,
            }],
            teams: vec![
                SyncTeam {
                    team: self.home.clone(),
                    stamp,
                },
                SyncTeam {
                    team: self.away.clone(),
                    stamp,
                },
            ],
            players: vec![SyncPlayer {
                player: self.player.clone(),
                stamp,
            }],
            facts: vec![self.fact(&self.phase, stamp), self.fact(&self.goal, stamp)],
        }
    }

    fn fact(&self, fact: &MatchFact, stamp: SyncStamp) -> SyncFact {
        SyncFact {
            match_id: self.match_.id,
            fact: fact.clone(),
            stamp,
        }
    }
}

/// `World` の試合を試合ファイルで受け取った写し: 試合・チーム・選手・fact の ID が全部違い、
/// 名前・中身・記録した時刻は同じ。
struct MatchFileCopy {
    match_id: MatchId,
    home: TeamId,
    away: TeamId,
    player: PlayerId,
    phase: FactId,
    goal: FactId,
    world: World,
}

impl MatchFileCopy {
    fn new(w: &World) -> MatchFileCopy {
        MatchFileCopy {
            match_id: MatchId(Uuid::from_u128(50)),
            home: TeamId(Uuid::from_u128(51)),
            away: TeamId(Uuid::from_u128(52)),
            player: PlayerId(Uuid::from_u128(53)),
            phase: FactId(Uuid::from_u128(60)),
            goal: FactId(Uuid::from_u128(61)),
            world: w.clone(),
        }
    }

    fn snapshot(&self, stamp: SyncStamp) -> SyncSnapshot {
        let w = &self.world;
        let mut match_ = w.match_.clone();
        match_.id = self.match_id;
        match_.home_team_id = self.home;
        match_.away_team_id = self.away;
        let mut phase = w.phase.clone();
        phase.id = self.phase;
        let mut scored = play_at_match(PlayEventKind::Goal, self.home, self.player, 600.0);
        scored.id = self.goal;
        SyncSnapshot {
            matches: vec![SyncMatch {
                match_,
                stamp,
                local_video: None,
            }],
            teams: vec![
                SyncTeam {
                    team: Team {
                        id: self.home,
                        name: w.home.name.clone(),
                    },
                    stamp,
                },
                SyncTeam {
                    team: Team {
                        id: self.away,
                        name: w.away.name.clone(),
                    },
                    stamp,
                },
            ],
            players: vec![SyncPlayer {
                player: Player {
                    id: self.player,
                    team_id: self.home,
                    ..w.player.clone()
                },
                stamp,
            }],
            facts: vec![
                SyncFact {
                    match_id: self.match_id,
                    fact: phase,
                    stamp,
                },
                SyncFact {
                    match_id: self.match_id,
                    fact: scored,
                    stamp,
                },
            ],
        }
    }
}

fn merged_with_duplicates(
    result: SyncReconcileResult,
) -> (
    SyncSnapshot,
    Vec<handball_toolkit::sync::SyncDuplicateGroup>,
) {
    match result {
        SyncReconcileResult::Merged {
            snapshot,
            duplicates,
        } => (snapshot, duplicates),
        SyncReconcileResult::Questions { questions } => panic!("問いが出た: {questions:?}"),
    }
}

fn goal(id: FactId, w: &World, secs: f64) -> MatchFact {
    let mut fact = play_at_match(PlayEventKind::Goal, w.home.id, w.player.id, secs);
    fact.id = id;
    fact
}

/// タイマーの試合 1 つ（区切り 0〜3600 秒・`scorer` の得点 1）と、その fact。
/// fact は（ID に使う数、記録した時刻）を区切り・得点の順に渡す。
fn timer_match_records(
    id: MatchId,
    (home, away): (TeamId, TeamId),
    scorer: PlayerId,
    [(phase_id, phase_at), (goal_id, goal_at)]: [(u128, DateTime<Utc>); 2],
    stamp: SyncStamp,
) -> (SyncMatch, Vec<SyncFact>) {
    let mut match_ = make_timer_match(home, away);
    match_.id = id;
    let mut phase = timer_phase(FactId(Uuid::from_u128(phase_id)), 0.0, 3600.0);
    phase.recorded_at = phase_at;
    let mut scored = play_at_match(PlayEventKind::Goal, home, scorer, 600.0);
    scored.id = FactId(Uuid::from_u128(goal_id));
    scored.recorded_at = goal_at;
    let facts = [phase, scored]
        .into_iter()
        .map(|fact| SyncFact {
            match_id: id,
            fact,
            stamp,
        })
        .collect();
    (
        SyncMatch {
            match_,
            stamp,
            local_video: None,
        },
        facts,
    )
}

fn live_match_ids(snapshot: &SyncSnapshot) -> Vec<MatchId> {
    snapshot
        .matches
        .iter()
        .filter(|m| !m.stamp.is_deleted())
        .map(|m| m.match_.id)
        .collect()
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

fn questions(result: SyncReconcileResult) -> Vec<SyncQuestion> {
    match result {
        SyncReconcileResult::Questions { questions } => questions,
        SyncReconcileResult::Merged { snapshot, .. } => panic!("問いが出なかった: {snapshot:?}"),
    }
}

fn set_title(snapshot: &mut SyncSnapshot, title: &str, stamp: SyncStamp) {
    snapshot.matches[0].match_.title = Some(title.to_owned());
    snapshot.matches[0].stamp = stamp;
}

fn set_fact_stamp(snapshot: &mut SyncSnapshot, id: FactId, stamp: SyncStamp) {
    let fact = snapshot.facts.iter_mut().find(|f| f.fact.id == id).unwrap();
    fact.stamp = stamp;
}

fn set_recorded_at(snapshot: &mut SyncSnapshot, id: FactId, at: DateTime<Utc>) {
    let fact = snapshot.facts.iter_mut().find(|f| f.fact.id == id).unwrap();
    fact.fact.recorded_at = at;
}

fn set_fact_note(snapshot: &mut SyncSnapshot, id: FactId, note: &str, stamp: SyncStamp) {
    let fact = snapshot.facts.iter_mut().find(|f| f.fact.id == id).unwrap();
    if let MatchFactPayload::Play(play) = &mut fact.fact.payload {
        play.note = Some(note.to_owned());
    }
    fact.stamp = stamp;
}

fn replace_fact(snapshot: &mut SyncSnapshot, fact: MatchFact, stamp: SyncStamp) {
    let entry = snapshot
        .facts
        .iter_mut()
        .find(|f| f.fact.id == fact.id)
        .unwrap();
    entry.fact = fact;
    entry.stamp = stamp;
}

fn note_of(fact: &MatchFact) -> Option<&str> {
    match &fact.payload {
        MatchFactPayload::Play(play) => play.note.as_deref(),
        _ => None,
    }
}

fn with_local_video(
    mut snapshot: SyncSnapshot,
    external_id: &str,
    cloud_identifier: Option<&str>,
) -> SyncSnapshot {
    let m = &mut snapshot.matches[0];
    m.match_.configuration = MatchConfiguration::Video(VideoSource {
        provider: VideoProvider::Local,
        external_id: external_id.to_owned(),
    });
    m.local_video = Some(LocalVideoIdentity {
        cloud_identifier: cloud_identifier.map(str::to_owned),
        duration_seconds: Some(3600.0),
    });
    // 動画モードの試合は videoClock の fact を持つので、比べる対象の fact を外しておく
    // （ここで見たいのは試合の中身と参照の扱いだけ）。
    snapshot.facts.clear();
    snapshot
}

fn external_id(match_: &Match) -> &str {
    match &match_.configuration {
        MatchConfiguration::Video(source) | MatchConfiguration::VideoHighlight(source) => {
            &source.external_id
        }
        MatchConfiguration::Timer { .. } => "",
    }
}
