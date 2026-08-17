package io.github.kinjoryura.handballtoolkit

import java.time.Instant
import java.util.UUID
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/**
 * シムアクセサの写経ミス検出（ADR 0004 決定 4「シムには最小限の単体テストを付ける」）。
 *
 * case の取り違えや null 判定の反転は Rust 側テストの守備範囲外 — シムは Rust を一度も
 * 呼ばないため。逆に言えばここで見るのは写経の正しさだけで、ドメイン規則は見ない
 * （規則はコアが持ち、シムは持たない）。
 *
 * ネイティブライブラリを触らないので JVM 単体テストとして回る（端末も .so も不要）。
 */
class ShimAccessorsTest {

    private val matchClock = MatchClock(12.5)
    private val videoClock = VideoClock(40.0)

    // ── FactAnchor ──

    @Test
    fun `matchClock anchor は matchClock だけを返す`() {
        val anchor: FactAnchor = FactAnchor.MatchClock(matchClock)
        assertEquals(FactAnchorKind.MATCH_CLOCK, anchor.kind)
        assertEquals(matchClock, anchor.matchClockOrNull)
        assertNull(anchor.videoClockOrNull)
        assertEquals(12.5, anchor.matchElapsedSeconds)
        assertNull(anchor.videoElapsedSeconds)
    }

    @Test
    fun `videoClock anchor は videoClock だけを返す`() {
        val anchor: FactAnchor = FactAnchor.VideoClock(videoClock)
        assertEquals(FactAnchorKind.VIDEO_CLOCK, anchor.kind)
        assertNull(anchor.matchClockOrNull)
        assertEquals(videoClock, anchor.videoClockOrNull)
        assertNull(anchor.matchElapsedSeconds)
        assertEquals(40.0, anchor.videoElapsedSeconds)
    }

    @Test
    fun `both anchor は両方の時計を返す`() {
        val anchor: FactAnchor = FactAnchor.Both(matchClock, videoClock)
        assertEquals(FactAnchorKind.BOTH, anchor.kind)
        assertEquals(matchClock, anchor.matchClockOrNull)
        assertEquals(videoClock, anchor.videoClockOrNull)
        assertEquals(12.5, anchor.matchElapsedSeconds)
        assertEquals(40.0, anchor.videoElapsedSeconds)
    }

    // ── MatchFact / ControlFact の代表 anchor ──

    @Test
    fun `play fact の代表 anchor は play の anchor`() {
        val anchor: FactAnchor = FactAnchor.MatchClock(matchClock)
        val fact = matchFact(MatchFactPayload.Play(PlayFact(kind = PlayEventKind.GOAL, anchor = anchor)))
        assertEquals(anchor, fact.anchor)
    }

    @Test
    fun `control fact の代表 anchor は startAnchor`() {
        val start: FactAnchor = FactAnchor.MatchClock(MatchClock(0.0))
        val end: FactAnchor = FactAnchor.MatchClock(MatchClock(1800.0))
        val control = ControlFact.PhaseStart(PhaseStartPayload(PhaseKind.REGULAR, start, end))
        val fact = matchFact(MatchFactPayload.Control(control))
        assertEquals(start, fact.anchor)
        assertEquals(start, control.startAnchor)
        assertEquals(end, control.endAnchor)
    }

    @Test
    fun `possession fact の代表 anchor は possession の anchor`() {
        val anchor: FactAnchor = FactAnchor.VideoClock(videoClock)
        val fact = matchFact(MatchFactPayload.Possession(PossessionFact(UUID.randomUUID(), anchor)))
        assertEquals(anchor, fact.anchor)
    }

    // ── 単一 anchor fact（R7 / R8 の対象。コアの MatchFact::single_anchor と同一挙動） ──

    @Test
    fun `play と possession は singleAnchor を持ち control は持たない`() {
        val point: FactAnchor = FactAnchor.MatchClock(matchClock)
        val play = matchFact(MatchFactPayload.Play(PlayFact(kind = PlayEventKind.GOAL, anchor = point)))
        val possession = matchFact(MatchFactPayload.Possession(PossessionFact(UUID.randomUUID(), point)))
        val control = matchFact(
            MatchFactPayload.Control(
                ControlFact.PhaseStart(
                    PhaseStartPayload(
                        PhaseKind.REGULAR,
                        FactAnchor.MatchClock(MatchClock(0.0)),
                        FactAnchor.MatchClock(MatchClock(1800.0)),
                    ),
                ),
            ),
        )
        assertEquals(point, play.singleAnchor)
        assertEquals(point, possession.singleAnchor)
        // range を持つ control は対象外。
        assertNull(control.singleAnchor)
    }

    @Test
    fun `stoppage の endAnchor は任意`() {
        val start: FactAnchor = FactAnchor.MatchClock(MatchClock(300.0))
        val open = ControlFact.Stoppage(StoppagePayload(StoppageKind.TIMEOUT, start))
        assertEquals(start, open.startAnchor)
        assertNull(open.endAnchor)

        val end: FactAnchor = FactAnchor.MatchClock(MatchClock(360.0))
        val closed = ControlFact.Stoppage(StoppagePayload(StoppageKind.TIMEOUT, start, end))
        assertEquals(end, closed.endAnchor)
    }

    // ── MatchConfiguration ──

    @Test
    fun `timer configuration`() {
        val configuration: MatchConfiguration = MatchConfiguration.Timer(1800.0)
        assertEquals(MatchConfigurationKind.TIMER, configuration.kind)
        assertEquals(CaptureMethod.MANUAL_CLOCK, configuration.captureMethod)
        assertNull(configuration.videoSource)
        assertEquals(1800.0, configuration.phaseDurationSecondsOrNull)
    }

    @Test
    fun `video configuration`() {
        val source = VideoSource(VideoProvider.YOUTUBE, "abc123")
        val configuration: MatchConfiguration = MatchConfiguration.Video(source)
        assertEquals(MatchConfigurationKind.VIDEO, configuration.kind)
        assertEquals(CaptureMethod.VIDEO, configuration.captureMethod)
        assertEquals(source, configuration.videoSource)
        assertNull(configuration.phaseDurationSecondsOrNull)
    }

    @Test
    fun `video highlight configuration`() {
        val source = VideoSource(VideoProvider.LOCAL, "local-1")
        val configuration: MatchConfiguration = MatchConfiguration.VideoHighlight(source)
        assertEquals(MatchConfigurationKind.VIDEO_HIGHLIGHT, configuration.kind)
        assertEquals(CaptureMethod.VIDEO, configuration.captureMethod)
        assertEquals(source, configuration.videoSource)
        assertNull(configuration.phaseDurationSecondsOrNull)
    }

    // ── projection の導出値 ──

    @Test
    fun `team summary の試投数と成功率`() {
        val line = TeamSummaryLine(UUID.randomUUID(), goals = 3L, shotMisses = 1L)
        assertEquals(4L, line.shotAttempts)
        assertEquals(0.75, line.scoringRate)
    }

    @Test
    fun `player stat の試投数と成功率`() {
        val line = PlayerStatLine(UUID.randomUUID(), goals = 1L, shotMisses = 3L)
        assertEquals(4L, line.shotAttempts)
        assertEquals(0.25, line.scoringRate)
    }

    @Test
    fun `試投 0 の成功率は null（0 除算ガード）`() {
        assertNull(TeamSummaryLine(UUID.randomUUID()).scoringRate)
        assertNull(PlayerStatLine(UUID.randomUUID()).scoringRate)

        val phase = phaseSummaryLine(homeGoals = 0L, homeShotMisses = 0L, awayGoals = 0L, awayShotMisses = 0L)
        assertNull(phase.homeRate)
        assertNull(phase.awayRate)
    }

    @Test
    fun `phase summary はホームとアウェイを取り違えない`() {
        val line = phaseSummaryLine(homeGoals = 5L, homeShotMisses = 5L, awayGoals = 1L, awayShotMisses = 3L)
        assertEquals(10L, line.homeAttempts)
        assertEquals(4L, line.awayAttempts)
        assertEquals(0.5, line.homeRate)
        assertEquals(0.25, line.awayRate)
    }

    @Test
    fun `score progression の diff は away 引く home`() {
        assertEquals(2L, ScoreProgressionPoint(60.0, homeScore = 3L, awayScore = 5L).diff)
        assertEquals(-2L, ScoreProgressionPoint(60.0, homeScore = 5L, awayScore = 3L).diff)
        assertEquals(0L, ScoreProgressionPoint(60.0, homeScore = 4L, awayScore = 4L).diff)
    }

    // ── RosterSelection ──

    @Test
    fun `Set ビューは往復する`() {
        val benched = setOf(UUID.randomUUID(), UUID.randomUUID())
        val outOfRoster = setOf(UUID.randomUUID())
        val selection = RosterSelection.of(benched, outOfRoster)
        assertEquals(benched, selection.benchedPlayerIdSet)
        assertEquals(outOfRoster, selection.outOfRosterPlayerIdSet)
    }

    @Test
    fun `of は Set の反復順に関わらず同じ値を作る`() {
        // data class の equals は List 比較なので、正準順に揃っていないと
        // 中身の等しい 2 値が不一致になる。
        val ids = List(16) { UUID.randomUUID() }
        val a = RosterSelection.of(ids.toSet())
        val b = RosterSelection.of(ids.reversed().toSet())
        assertEquals(a, b)
    }

    @Test
    fun `正準順はバイト昇順（UUID の符号付き比較ではない）`() {
        // 最上位ビットが立つ UUID は java.util.UUID.compareTo では負数として先に来る。
        // 正準順は Rust の BTreeSet<Uuid>（バイト昇順）と一致していなければならない。
        val low = UUID.fromString("00000000-0000-4000-8000-000000000001")
        val high = UUID.fromString("ffffffff-0000-4000-8000-000000000001")
        val selection = RosterSelection.of(setOf(high, low))
        assertEquals(listOf(low, high), selection.benchedPlayerIds)
    }

    @Test
    fun `of の既定値は空`() {
        val selection = RosterSelection.of()
        assertEquals(emptyList(), selection.benchedPlayerIds)
        assertEquals(emptyList(), selection.outOfRosterPlayerIds)
    }

    // ── TeamOption ──

    @Test
    fun `既存チームがある候補の id は そのチームの id`() {
        val team = Team(UUID.randomUUID(), "青葉ハンドボールクラブ")
        assertEquals(team.id, teamOption(team).id)
    }

    @Test
    fun `新規作成候補の id は固定値`() {
        assertEquals(TeamOption.NEW_TEAM_OPTION_ID, teamOption(null).id)
        // iOS シムと同じ値であること（両シェルで候補リストの識別子が一致する）。
        assertEquals(UUID.fromString("00000000-0000-0000-0000-00000000FFFF"), TeamOption.NEW_TEAM_OPTION_ID)
    }

    // ── helper ──

    private fun matchFact(payload: MatchFactPayload) =
        MatchFact(UUID.randomUUID(), Instant.EPOCH, payload)

    private fun phaseSummaryLine(
        homeGoals: Long,
        homeShotMisses: Long,
        awayGoals: Long,
        awayShotMisses: Long,
    ) = PhaseSummaryLine(
        UUID.randomUUID(),
        PhaseKind.REGULAR,
        0,
        homeGoals,
        homeShotMisses,
        awayGoals,
        awayShotMisses,
    )

    private fun teamOption(existing: Team?) =
        TeamOption(existing, emptyList(), emptyList(), emptyList(), emptyList())
}
