package com.example.handballshell.db

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query

/**
 * サンプルなので DAO は 1 本にまとめている。**この層に「保存してよいか」の判断は無い**
 * — 検証・参照整合判定・保存順序はすべてコア側（ADR 0005）。ここは素朴な CRUD だけ。
 */
@Dao
interface ShellDao {

    // ── team / player ──

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertTeam(row: TeamRow)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertTeams(rows: List<TeamRow>)

    @Query("DELETE FROM team WHERE id = :teamId")
    suspend fun deleteTeam(teamId: String)

    @Query("SELECT * FROM team WHERE id = :teamId")
    suspend fun findTeam(teamId: String): TeamRow?

    @Query("SELECT * FROM team ORDER BY name")
    suspend fun allTeams(): List<TeamRow>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertPlayer(row: PlayerRow)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertPlayers(rows: List<PlayerRow>)

    @Query("DELETE FROM player WHERE id = :playerId")
    suspend fun deletePlayer(playerId: String)

    /** delete_team の cascade（trait 実装内に残す判断 — ADR 0005 決定 2）。 */
    @Query("DELETE FROM player WHERE teamId = :teamId")
    suspend fun deletePlayersOfTeam(teamId: String)

    @Query("SELECT * FROM player WHERE teamId IN (:teamIds) ORDER BY jerseyNumber, name")
    suspend fun playersOfTeams(teamIds: List<String>): List<PlayerRow>

    // ── match ──

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertMatch(row: MatchRow)

    @Query("SELECT * FROM match WHERE id = :matchId")
    suspend fun findMatch(matchId: String): MatchRow?

    /** seed 済みかの判定に使うだけ（サンプル都合）。 */
    @Query("SELECT * FROM match ORDER BY dateEpochSecond LIMIT 1")
    suspend fun firstMatch(): MatchRow?

    @Query("DELETE FROM match WHERE id = :matchId")
    suspend fun deleteMatch(matchId: String)

    @Query("SELECT COUNT(*) FROM match WHERE homeTeamId = :teamId OR awayTeamId = :teamId")
    suspend fun countMatchesReferencingTeam(teamId: String): Int

    // ── fact ──

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertFact(row: FactRow)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertFacts(rows: List<FactRow>)

    @Query("DELETE FROM fact WHERE id = :factId AND matchId = :matchId")
    suspend fun deleteFact(matchId: String, factId: String)

    @Query("DELETE FROM fact WHERE matchId = :matchId")
    suspend fun deleteFactsOfMatch(matchId: String)

    /**
     * **永続化順**で返す（コアの `persistence_order` と同じ規約 — 累積秒 → recordedAt → id）。
     * validators の入力契約が「facts は永続化順でソート済み」を要求するため、
     * 読み出し順を合わせるのはシェルの責務。
     *
     * 累積秒は「matchClock があればそれ、無ければ videoClock、どちらも無ければ末尾」。
     * 第 1 キーで NULL 群を後ろへ寄せてから COALESCE で比較する（`?? .infinity` と同じ扱い）。
     */
    @Query(
        """
        SELECT * FROM fact WHERE matchId = :matchId
        ORDER BY
          (startMatchSeconds IS NULL AND startVideoSeconds IS NULL) ASC,
          COALESCE(startMatchSeconds, startVideoSeconds) ASC,
          recordedAtEpochSecond ASC,
          recordedAtNano ASC,
          id ASC
        """,
    )
    suspend fun factLog(matchId: String): List<FactRow>

    @Query("SELECT COUNT(*) FROM fact WHERE playerId = :playerId OR relatedPlayerId = :playerId")
    suspend fun countFactsReferencingPlayer(playerId: String): Int
}
