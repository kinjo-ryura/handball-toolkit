// FFI 本境界の検証ハーネス（ADR 0004。旧 #49 PoC の JSON 境界 smoke を改修）。
// XCFramework + 生成 Swift バインディング経由で、生成型の構築 → projection /
// validator / SegmentResolver（object ハンドル）の一巡を iOS シミュレータ内で確認する。
// ビルド・実行は同ディレクトリの run.sh。
import Foundation

var failures = 0
func check(_ label: String, _ condition: Bool) {
    print("\(condition ? "OK" : "NG"): \(label)")
    if !condition { failures += 1 }
}

// 1) 疎通確認の最小関数
print("== toolkitVersion() ==")
print(toolkitVersion())

// 2) 生成型で動画モードの試合を組み立てる（UUID / Date は custom type 写像の確認を兼ねる）
let homeTeam = UUID()
let awayTeam = UUID()
let match = Match(
    id: UUID(),
    title: "FFI 本境界 smoke",
    date: Date(),
    homeTeamId: homeTeam,
    awayTeamId: awayTeam,
    configuration: .video(VideoSource(provider: .youtube, externalId: "poc")),
    rosterSelection: RosterSelection(benchedPlayerIds: [], outOfRosterPlayerIds: []),
    isHomeOnLeft: true
)

func playFact(kind: PlayEventKind, team: UUID, videoSeconds: Double) -> MatchFact {
    MatchFact(
        id: UUID(),
        recordedAt: Date(),
        payload: .play(PlayFact(
            kind: kind,
            teamId: team,
            playerId: nil,
            relatedPlayerId: nil,
            anchor: .videoClock(VideoClock(elapsedSeconds: videoSeconds)),
            title: nil,
            note: nil
        ))
    )
}

let facts: [MatchFact] = [
    MatchFact(
        id: UUID(),
        recordedAt: Date(),
        payload: .control(.phaseStart(PhaseStartPayload(
            kind: .regular,
            startAnchor: .videoClock(VideoClock(elapsedSeconds: 10)),
            endAnchor: .videoClock(VideoClock(elapsedSeconds: 70))
        )))
    ),
    playFact(kind: .goal, team: homeTeam, videoSeconds: 30),
    playFact(kind: .goal, team: awayTeam, videoSeconds: 50),
]

// 3) validators: 正常 log は空、負の videoClock は blocking issue
print("\n== validators ==")
check("validateMatch は空", validateMatch(match: match).isEmpty)
check("validateFactLog は空", validateFactLog(facts: facts, match: match).isEmpty)
let badFact = playFact(kind: .goal, team: homeTeam, videoSeconds: -1)
let issues = validateAppend(fact: badFact, existingFacts: facts, match: match, roster: nil)
check("負の videoClock は blocking", issues.contains(.fact(FactValidationError.negativeVideoClock)))

// 4) projection: timeline → summary（1-1）
print("\n== projections ==")
let timeline = buildTimeline(match: match, facts: facts)
check("resolvedFacts は 3 件", timeline.resolvedFacts.count == 3)
let summary = buildSummaryWithTimeline(match: match, timeline: timeline)
check("スコアは 1-1", summary.homeScore == 1 && summary.awayScore == 1)
check("phase 別 stats は 1 phase", summary.phaseSummaries.count == 1)

// 5) SegmentResolver（object ハンドル）: 時刻変換 + record 内ハンドル再利用
print("\n== SegmentResolver ==")
let resolver = SegmentResolver.build(facts: facts)
let resolved = resolver.resolveMatchClock(video: VideoClock(elapsedSeconds: 40))
check("video 40s → match 30s", resolved?.elapsedSeconds == 30)
check("timeline.resolver ハンドルも同じ変換",
      timeline.resolver.resolveMatchClock(video: VideoClock(elapsedSeconds: 40))?.elapsedSeconds == 30)

// 6) live projection（2Hz tick 経路の形）
let live = buildLiveMatchVideoMode(
    match: match,
    timeline: timeline,
    currentVideoClock: VideoClock(elapsedSeconds: 40)
)
check("video 40s は playing", live.timerState == .playing)
check("phase index 0", live.currentPhaseIndex == 0)

// 7) sample_dto: parse → requiredIdCount → convert → projection → export → encode → 再 parse
print("\n== sample_dto ==")
let sampleJson = """
{
  "schemaVersion": 2,
  "match": {
    "displayName": "FFI サンプル",
    "date": "2026-01-01T00:00:00Z",
    "configuration": {"kind": "timer", "timer": {"phaseDurationSeconds": 1800}}
  },
  "teams": {
    "home": {"key": "home", "name": "Tigers", "players": [{"key": "p1", "name": "Alice", "jerseyNumber": 7}]},
    "away": {"key": "away", "name": "Falcons", "players": []}
  },
  "facts": [
    {
      "recordedAt": "2026-01-01T00:10:00Z",
      "payload": {
        "kind": "control",
        "control": {
          "kind": "phaseStart",
          "phaseStart": {"kind": "regular"},
          "anchor": {"kind": "matchClock", "matchClock": {"elapsedSeconds": 0}, "endMatchElapsedSeconds": 1800}
        }
      }
    },
    {
      "recordedAt": "2026-01-01T00:11:00Z",
      "payload": {
        "kind": "play",
        "play": {"kind": "goal", "teamKey": "home", "playerKey": "p1", "anchor": {"kind": "matchClock", "matchClock": {"elapsedSeconds": 60}}}
      }
    }
  ]
}
"""
do {
    let dto = try parseSampleMatch(json: sampleJson)
    let required = sampleMatchRequiredIdCount(dto: dto)
    check("必要 ID 数は 6", required == 6) // teams 2 + Alice + match + factID 無し fact 2

    // ID 不足は構造化エラーで拒否される（事前生成 Vec<Uuid> 方式 — ADR 0004 決定 2）
    do {
        _ = try convertSampleMatch(slug: "smoke", dto: dto, configurationOverride: nil, newIds: [])
        check("ID 不足は throw", false)
    } catch SampleDtoError.InsufficientNewIds(let requiredCount, let provided) {
        check("ID 不足は InsufficientNewIds", requiredCount == 6 && provided == 0)
    }

    let ids = (0..<required).map { _ in UUID() }
    let conversion = try convertSampleMatch(slug: "smoke", dto: dto, configurationOverride: nil, newIds: ids)
    check("convert: facts 2 件", conversion.facts.count == 2)
    check("convert: playerKey 逆写像あり", conversion.playersByKey["p1"] != nil)

    // 変換結果はそのまま projection に流せる
    let summary = buildSummary(match: conversion.match, facts: conversion.facts)
    check("サンプルのスコアは 1-0", summary.homeScore == 1 && summary.awayScore == 0)

    // export → encode → 再 parse の round-trip
    let homePlayers = conversion.players.filter { $0.teamId == conversion.homeTeam.id }
    let awayPlayers = conversion.players.filter { $0.teamId == conversion.awayTeam.id }
    let exported = exportSampleMatch(
        match: conversion.match, homeTeam: conversion.homeTeam, awayTeam: conversion.awayTeam,
        homePlayers: homePlayers, awayPlayers: awayPlayers, facts: conversion.facts)
    let encoded = encodeSampleMatch(dto: exported)
    let reparsed = try parseSampleMatch(json: encoded)
    check("round-trip: displayName 保持", reparsed.match.displayName == "FFI サンプル")
    check("round-trip: factID 付与済み", reparsed.facts.count == 2 && reparsed.facts.allSatisfy { $0.factId != nil })
    let slug = sampleMatchDefaultSlug(match: conversion.match, homeTeam: conversion.homeTeam, awayTeam: conversion.awayTeam)
    check("defaultSlug 生成", slug == "2026-01-01-tigers-vs-falcons")
} catch {
    check("sample_dto 経路で例外なし (実際: \(error))", false)
}

// 8) sample_dto エラー経路（Swift throws への写像）
do {
    _ = try parseSampleMatch(json: "{")
    check("不正 JSON は throw", false)
} catch is SampleDtoError {
    check("不正 JSON は SampleDtoError", true)
} catch {
    check("不正 JSON は SampleDtoError (実際: \(error))", false)
}

if failures > 0 {
    print("\nNG: \(failures) 件失敗")
    exit(1)
}
print("\n本境界 smoke 完了: 生成型 → projection / validator / object ハンドル / sample_dto の一巡を確認")
