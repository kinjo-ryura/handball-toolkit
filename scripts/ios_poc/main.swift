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

if failures > 0 {
    print("\nNG: \(failures) 件失敗")
    exit(1)
}
print("\n本境界 smoke 完了: 生成型 → projection / validator / object ハンドルの一巡を確認")
