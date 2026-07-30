package com.example.handballshell

import com.example.handballshell.db.ShellDatabase
import java.time.Instant
import java.util.UUID
import uniffi.handball_toolkit.Match
import uniffi.handball_toolkit.MatchConfiguration
import uniffi.handball_toolkit.Player
import uniffi.handball_toolkit.RosterSelection
import uniffi.handball_toolkit.Team
import uniffi.handball_toolkit.recordSaveMatch
import uniffi.handball_toolkit.recordSavePlayer
import uniffi.handball_toolkit.recordSaveTeam

private const val KEY_SEEDED_MATCH_ID = "seededMatchId"

class SeedIds(
    val matchId: UUID,
    val homeTeamId: UUID,
    val awayTeamId: UUID,
    val homePlayerIds: List<UUID>,
    val awayPlayerIds: List<UUID>,
)

/**
 * 初回起動時の下ごしらえ。**書き込みはすべてコア入口経由**にしてある
 * （`record_save_team` / `record_save_player` / `record_save_match`）。
 * repository を直接叩かないのは、iOS 側で可視性遮断（ADR 0005 決定 3）が保証している
 * 「コアを通らない書き込み経路は存在しない」を Kotlin でも手で守るため。
 *
 * Kotlin には Swift のような「プロトコルから write メソッドを外す」遮断が効かない
 * （trait 実装クラスがそのまま公開される）。実プロダクトのシェルでは repository 実装を
 * internal にして composition root だけが知る形にするとよい。
 */
suspend fun ensureSeed(
    prefs: android.content.SharedPreferences,
    db: ShellDatabase,
    matchRepo: RoomMatchWriteRepository,
    teamRepo: RoomTeamWriteRepository,
): SeedIds {
    // seed した試合の id を覚えておく。「最初の 1 件」で引くと、より古い日付の試合を
    // import した瞬間にそちらを掴んでしまう。
    prefs.getString(KEY_SEEDED_MATCH_ID, null)
        ?.let { db.dao().findMatch(it) }
        ?.let { existing ->
        val home = db.dao().playersOfTeams(listOf(existing.homeTeamId)).map { UUID.fromString(it.id) }
        val away = db.dao().playersOfTeams(listOf(existing.awayTeamId)).map { UUID.fromString(it.id) }
        return SeedIds(
            matchId = UUID.fromString(existing.id),
            homeTeamId = UUID.fromString(existing.homeTeamId),
            awayTeamId = UUID.fromString(existing.awayTeamId),
            homePlayerIds = home,
            awayPlayerIds = away,
        )
    }

    // ID / 時刻はシェルが発行する（コアは生成しない — 設計不変条件 2）。
    val homeTeam = Team(id = UUID.randomUUID(), name = "青葉ハンドボールクラブ")
    val awayTeam = Team(id = UUID.randomUUID(), name = "白鳥ハンドボールクラブ")
    recordSaveTeam(teamRepo, homeTeam)
    recordSaveTeam(teamRepo, awayTeam)

    fun roster(team: Team, names: List<String>): List<Player> =
        names.mapIndexed { index, name ->
            Player(
                id = UUID.randomUUID(),
                teamId = team.id,
                name = name,
                jerseyNumber = (index + 1).toLong(),
                photo = null,
            )
        }

    val homePlayers = roster(homeTeam, listOf("佐藤", "鈴木", "高橋"))
    val awayPlayers = roster(awayTeam, listOf("田中", "伊藤", "渡辺"))
    (homePlayers + awayPlayers).forEach { recordSavePlayer(teamRepo, it) }

    val match = Match(
        id = UUID.randomUUID(),
        title = "サンプル試合",
        date = Instant.now(),
        homeTeamId = homeTeam.id,
        awayTeamId = awayTeam.id,
        // タイマーモード（規定長 30 分）。動画なしのフル試合。
        configuration = MatchConfiguration.Timer(phaseDurationSeconds = 1_800.0),
        rosterSelection = RosterSelection(
            benchedPlayerIds = emptyList(),
            outOfRosterPlayerIds = emptyList(),
        ),
        isHomeOnLeft = true,
    )
    recordSaveMatch(matchRepo, match)
    prefs.edit().putString(KEY_SEEDED_MATCH_ID, match.id.toString()).apply()

    return SeedIds(
        matchId = match.id,
        homeTeamId = homeTeam.id,
        awayTeamId = awayTeam.id,
        homePlayerIds = homePlayers.map { it.id },
        awayPlayerIds = awayPlayers.map { it.id },
    )
}
