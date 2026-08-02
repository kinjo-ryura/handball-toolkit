package com.example.handballshell

import android.app.Activity
import android.os.Bundle
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.example.handballshell.db.ShellDatabase
import com.example.handballshell.db.toDomain
import java.time.Instant
import java.util.UUID
import kotlin.system.measureNanoTime
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import io.github.kinjoryura.handballtoolkit.CoreWriteException
import io.github.kinjoryura.handballtoolkit.DomainValidationIssue
import io.github.kinjoryura.handballtoolkit.FactAnchor
import io.github.kinjoryura.handballtoolkit.MatchClock
import io.github.kinjoryura.handballtoolkit.MatchConfiguration
import io.github.kinjoryura.handballtoolkit.NewFactStamp
import io.github.kinjoryura.handballtoolkit.PlayEventKind
import io.github.kinjoryura.handballtoolkit.SegmentResolver
import io.github.kinjoryura.handballtoolkit.VideoClock
import io.github.kinjoryura.handballtoolkit.buildPlayFact
import io.github.kinjoryura.handballtoolkit.buildSummary
import io.github.kinjoryura.handballtoolkit.commitSampleMatchImport
import io.github.kinjoryura.handballtoolkit.countPhaseCompletionFacts
import io.github.kinjoryura.handballtoolkit.defaultImportDecisions
import io.github.kinjoryura.handballtoolkit.newImportTeamOption
import io.github.kinjoryura.handballtoolkit.parseSampleMatch
import io.github.kinjoryura.handballtoolkit.recordAppendFact
import io.github.kinjoryura.handballtoolkit.recordDeleteTeam
import io.github.kinjoryura.handballtoolkit.recordFactWithPhaseCompletion
import io.github.kinjoryura.handballtoolkit.sampleImportRequiredIdCount
import io.github.kinjoryura.handballtoolkit.toolkitVersion

/**
 * write 経路をひととおり踏むだけの最小シェル。UI の作り込みはしていない
 * （このサンプルの目的は「シェル契約が一目で分かること」— README 参照）。
 */
class MainActivity : Activity() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private lateinit var log: TextView

    private val db by lazy { ShellDatabase.get(this) }
    private val matchRepo by lazy { RoomMatchWriteRepository(db) }
    private val teamRepo by lazy { RoomTeamWriteRepository(db) }
    private val importRepo by lazy { RoomImportWriteRepository(db) }

    private lateinit var seed: SeedIds

    /** 記録位置（matchClock 累積秒）。記録するたびに 60 秒進める。 */
    private var clockSeconds = 60.0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildUi())
        scope.launch {
            val prefs = getSharedPreferences("shell", MODE_PRIVATE)
            seed = withContext(Dispatchers.IO) { ensureSeed(prefs, db, matchRepo, teamRepo) }
            append("toolkit ${toolkitVersion()} / seed 済み")
            showSummary()
        }
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun buildUi(): ViewGroup {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(24, 24, 24, 24)
        }
        fun button(label: String, action: suspend () -> Unit) {
            root.addView(
                Button(this).apply {
                    text = label
                    isAllCaps = false
                    setOnClickListener { run(action) }
                },
            )
        }

        button("① ゴールを記録（phase 自動補完つき）") { recordGoalWithPhaseCompletion() }
        button("② シュート失敗を記録（record_append_fact）") { recordShotMissed() }
        button("③ 動画時刻の anchor で記録 → ValidationFailed") { recordInvalidAnchor() }
        button("④ 使用中チームを削除 → TeamInUse") { deleteTeamInUse() }
        button("⑤ サンプル試合を import（atomic / commit_import）") { importSampleMatch() }
        button("⑥ 2Hz 相当パスを実測（SegmentResolver）") { benchmarkHotPath() }

        log = TextView(this).apply {
            textSize = 12f
            setTextIsSelectable(true)
        }
        root.addView(log)

        return ScrollView(this).apply {
            // targetSdk 35 以降はアプリが既定で edge-to-edge になり、内容がシステムバーの
            // 裏まで描かれる。インセット分の padding を入れないと先頭のボタンが隠れる。
            fitsSystemWindows = true
            addView(root)
        }
    }

    private fun run(action: suspend () -> Unit) {
        scope.launch {
            try {
                action()
            } catch (e: CoreWriteException) {
                // ADR 0002: コアは構造化エラーだけを返す。**文言はシェルが持つ**。
                append("✗ ${describe(e)}")
            } catch (e: Exception) {
                append("✗ 予期しない失敗: $e")
            }
        }
    }

    // ── ID / 時刻の供給（コアは now() / UUID を持たない — ADR 0005 決定 4）──

    private fun newStamp() = NewFactStamp(id = UUID.randomUUID(), recordedAt = Instant.now())

    // ── 記録操作 ──

    /**
     * タイマーモードの本命経路。必要なスタンプ数を**コアに数えさせ**、その数だけ生成して渡す
     * （消費順の知識をシェルへ漏らさない）。phase がまだ無ければコアが自動補完する。
     */
    private suspend fun recordGoalWithPhaseCompletion() {
        val anchor = FactAnchor.MatchClock(MatchClock(clockSeconds))
        val fact = buildPlayFact(
            stamp = newStamp(),
            kind = PlayEventKind.GOAL,
            teamId = seed.homeTeamId,
            playerId = seed.homePlayerIds.first(),
            anchor = anchor,
            title = null,
            note = null,
        )
        val required = countPhaseCompletionFacts(matchRepo, seed.matchId, fact)
        val stamps = List(required) { newStamp() }
        recordFactWithPhaseCompletion(matchRepo, seed.matchId, fact, stamps)

        append("✓ ゴール @ ${clockSeconds.toInt()}s（補完 phase ${required} 件）")
        clockSeconds += 60.0
        showSummary()
    }

    private suspend fun recordShotMissed() {
        val fact = buildPlayFact(
            stamp = newStamp(),
            kind = PlayEventKind.SHOT_MISSED,
            teamId = seed.awayTeamId,
            playerId = seed.awayPlayerIds.first(),
            anchor = FactAnchor.MatchClock(MatchClock(clockSeconds)),
            title = null,
            note = null,
        )
        recordAppendFact(matchRepo, seed.matchId, fact)
        append("✓ シュート失敗 @ ${clockSeconds.toInt()}s")
        clockSeconds += 60.0
        showSummary()
    }

    /**
     * configuration と噛み合わない anchor で記録して blocking validation を踏む。
     * タイマーモードが許すのは matchClock anchor だけなので、videoClock は必ず拒否される
     * （発火しない = DB は変わらない）。
     */
    private suspend fun recordInvalidAnchor() {
        val fact = buildPlayFact(
            stamp = newStamp(),
            kind = PlayEventKind.GOAL,
            teamId = seed.homeTeamId,
            playerId = seed.homePlayerIds.first(),
            anchor = FactAnchor.VideoClock(VideoClock(120.0)),
            title = null,
            note = null,
        )
        recordAppendFact(matchRepo, seed.matchId, fact)
        append("… 拒否されるはずだった（記録されてしまった）")
    }

    /** 参照整合の判定は**コア**が持つ。シェルはカウントを返しただけ。 */
    private suspend fun deleteTeamInUse() {
        recordDeleteTeam(teamRepo, seed.homeTeamId)
        append("… 拒否されるはずだった（削除されてしまった）")
    }

    /** import は 1 トランザクション（全成功 or 1 件も保存しない）。 */
    private suspend fun importSampleMatch() {
        val json = assets.open("sample-match.json").bufferedReader().use { it.readText() }
        val dto = parseSampleMatch(json)
        // 既存チームへの統合はせず、常に新規作成する（UI の選択肢はサンプルでは省略）。
        val decisions = defaultImportDecisions(
            newImportTeamOption(listOf(dto.teams.home.key)),
            newImportTeamOption(listOf(dto.teams.away.key)),
        )
        val required = sampleImportRequiredIdCount(dto, decisions)
        val ids = List(required) { UUID.randomUUID() }
        val outcome = commitSampleMatchImport(matchRepo, importRepo, dto, decisions, ids)
        append(
            "✓ import 完了: teams +${outcome.teamsCreated} / players +${outcome.playersCreated} " +
                "/ facts ${dto.facts.size} 件を 1 トランザクションで保存",
        )
    }

    // ── 2Hz ホットパスの実測（ADR 0004 決定 5 の検証）──

    private suspend fun benchmarkHotPath() {
        val report = withContext(Dispatchers.Default) { measureHotPath() }
        append(report)
        // 画面が狭いので logcat にも出す（adb logcat -s HandballShell）。
        android.util.Log.i("HandballShell", report)
    }

    /**
     * 2Hz ホットパスの実測（ADR 0004 決定 5 の前提「FFI 越えは µs オーダー」を Android で検証する）。
     *
     * 実試合の規模に寄せるため、DB の fact に加えて合成 fact でも測る。
     * **release ビルドで測ること** — debuggable なプロセスは -Xcheck:jni が入って桁が変わる。
     */
    private suspend fun measureHotPath(): String {
        val stored = db.dao().factLog(seed.matchId.toString()).map { it.toDomain() }
        if (stored.isEmpty()) return "先に fact を記録してください"

        // 実試合相当（前後半で 300 件程度）の合成ログ。phase fact は既存のものを使う。
        val synthetic = stored + (1..300).map { i ->
            buildPlayFact(
                stamp = NewFactStamp(UUID.randomUUID(), Instant.now()),
                kind = if (i % 2 == 0) PlayEventKind.GOAL else PlayEventKind.SHOT_MISSED,
                teamId = if (i % 2 == 0) seed.homeTeamId else seed.awayTeamId,
                playerId = null,
                anchor = FactAnchor.MatchClock(MatchClock((i * 5).toDouble())),
                title = null,
                note = null,
            )
        }
        val match = db.dao().findMatch(seed.matchId.toString())!!.toDomain()

        val n = 2_000
        return buildString {
            appendLine("── 2Hz パス実測（release / ${n} 回平均）──")

            for (facts in listOf(stored, synthetic)) {
                SegmentResolver.build(facts).close() // ウォームアップ
                val buildNanos = measureNanoTime {
                    repeat(100) { SegmentResolver.build(facts).close() }
                } / 100

                // object ハンドルは Rust の Arc を保持する。AutoCloseable なので明示的に手放す。
                SegmentResolver.build(facts).use { resolver ->
                    repeat(500) { resolver.phaseKind(it.toDouble()) } // ウォームアップ
                    // 引数 record + 戻り Option（RustBuffer 2 往復）
                    val resolveNanos = measureNanoTime {
                        repeat(n) { resolver.resolveMatchClock(VideoClock((it % 600).toDouble())) }
                    } / n
                    // 引数 scalar + 戻り Option（RustBuffer 1 往復）
                    val phaseNanos = measureNanoTime {
                        repeat(n) { resolver.phaseKind((it % 600).toDouble()) }
                    } / n
                    // 材料化（表を 1 回引いて以後 Kotlin 側で解決する案の取得コスト）
                    val segmentsNanos = measureNanoTime { repeat(100) { resolver.allSegments() } } / 100
                    // 粗い呼び出し 1 本（fact 列を毎回マーシャリングする）
                    val summaryNanos = measureNanoTime { repeat(100) { buildSummary(match, facts) } } / 100

                    appendLine("[facts ${facts.size} 件]")
                    appendLine("  SegmentResolver.build : ${buildNanos / 1000} µs")
                    appendLine("  resolveMatchClock     : ${resolveNanos / 1000} µs/呼び出し")
                    appendLine("  phaseKind             : ${phaseNanos / 1000} µs/呼び出し")
                    appendLine("  allSegments           : ${segmentsNanos / 1000} µs")
                    appendLine("  buildSummary（粗い）   : ${summaryNanos / 1000} µs")
                }
            }
        }
    }

    // ── 表示 ──

    private suspend fun showSummary() {
        val match = withContext(Dispatchers.IO) {
            db.dao().findMatch(seed.matchId.toString())!!.toDomain()
        }
        val facts = withContext(Dispatchers.IO) {
            db.dao().factLog(seed.matchId.toString()).map { it.toDomain() }
        }
        val summary = buildSummary(match, facts)
        val phase = (match.configuration as? MatchConfiguration.Timer)?.phaseDurationSeconds
        append(
            "  スコア ${summary.homeScore} - ${summary.awayScore}" +
                "（fact ${facts.size} 件 / phase 規定長 ${phase?.toInt()}s）",
        )
    }

    /**
     * 構造化エラー → ユーザー向け文言。**この写像はシェルの責務**であり、コアは
     * コードとパラメータしか返さない（ADR 0002）。実プロダクトでは
     * DomainValidationIssue の (scope, code) ごとに文言表を持つ。
     */
    private fun describe(e: CoreWriteException): String =
        when (e) {
            is CoreWriteException.ValidationFailed ->
                "記録できません: " + e.issues.joinToString("; ") { describeIssue(it) }
            is CoreWriteException.TeamInUse ->
                "このチームは ${e.matchCount} 試合で使われているため削除できません"
            is CoreWriteException.PlayerInUse ->
                "この選手は ${e.factCount} 件の記録で参照されているため削除できません"
            is CoreWriteException.InsufficientNewIds ->
                "ID の供給が不足しました（必要 ${e.required} / 渡した ${e.provided}）。やり直してください"
            is CoreWriteException.Repository ->
                "保存に失敗しました（開発者向け: ${e.detail}）"
            is CoreWriteException.MigrationPlanInfeasible ->
                "動画への移行を計画できませんでした（開発者向け: ${e.detail}）"
            is CoreWriteException.ImportDecodeFailed ->
                "取り込むデータを解釈できませんでした（開発者向け: ${e.detail}）"
        }

    // サンプルなので issue は toString で出す。実シェルは (scope, code) で文言を引く。
    private fun describeIssue(issue: DomainValidationIssue): String =
        when (issue) {
            is DomainValidationIssue.Match -> issue.v1.toString()
            is DomainValidationIssue.Configuration -> issue.v1.toString()
            is DomainValidationIssue.Fact -> issue.v1.toString()
            is DomainValidationIssue.Timeline -> issue.v1.toString()
        }

    private fun append(line: String) {
        log.append(line + "\n")
    }
}
