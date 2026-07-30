package com.example.handballshell

import androidx.room.withTransaction
import com.example.handballshell.db.ShellDatabase
import com.example.handballshell.db.toDomain
import com.example.handballshell.db.toRow
import java.util.UUID
import uniffi.handball_toolkit.CoreWriteException
import uniffi.handball_toolkit.ImportWriteBatch
import uniffi.handball_toolkit.ImportWriteRepository
import uniffi.handball_toolkit.Match
import uniffi.handball_toolkit.MatchFact
import uniffi.handball_toolkit.MatchWriteRepository
import uniffi.handball_toolkit.Player
import uniffi.handball_toolkit.PlayerTeamRef
import uniffi.handball_toolkit.Team
import uniffi.handball_toolkit.TeamWriteRepository

// ─────────────────────────────────────────────────────────────────────────────
//  シェル契約のすべて: 3 trait / 15 メソッド
// ─────────────────────────────────────────────────────────────────────────────
//
//  コアが所有するもの（ここには書かない）:
//    - 「保存してよいか」の検証（validate_append / update / delete）
//    - 参照整合の判定（使用中チーム / 選手の削除拒否）
//    - 何をどの順に保存するかの計画（phase 自動補完・migrate・import の保存順）
//
//  シェルが所有するもの（ここに書くもの）:
//    - DB ハンドルとトランザクション境界（コアは DB を持たない）
//    - read は「検証入力の最小セット」だけ供給する
//    - write は**検証なしの素朴 CRUD**
//    - 例外を CoreWriteException.Repository へ写像する
//
//  さらにシェル所有だが別の場所にあるもの:
//    - ID / 時刻の供給（コアは now() / UUID を持たない）→ MainActivity の stamp 生成
//    - ユーザー向け文言（コアは構造化エラーのみ返す）→ MainActivity の describe()
//
//  ADR 0005（write orchestration）/ ADR 0002（エラー体系）が正典。

/**
 * 例外を構造化エラーへ写像する。`CoreWriteException` はそのまま通す（コアが投げさせた
 * ものなので二重包装しない）。
 *
 * ここで包まなかった例外も uniffi が拾って `Repository` に畳んでくれる（生成コードの
 * `uniffiTraitInterfaceCallAsyncWithError` が `kotlin.Exception` を catch し、
 * Rust 側の `From<UnexpectedUniFFICallbackError>` が受ける）。ただし **catch されるのは
 * `Exception` までで `Error` は素通り**し、GlobalScope の未捕捉例外としてプロセスを
 * 落とすため、シェル側でも明示的に写像しておくほうが安全。
 */
private inline fun <T> mapRepositoryFailure(block: () -> T): T =
    try {
        block()
    } catch (e: CoreWriteException) {
        throw e
    } catch (e: Exception) {
        throw CoreWriteException.Repository("Room: ${e::class.simpleName}: ${e.message}")
    }

/**
 * 試合スコープの write repository（8 メソッド: read 3 + write 5）。
 *
 * 生成 Kotlin では `suspend fun` の interface になる。**coroutine scope を渡す必要は無い**
 * — uniffi の生成コードが `GlobalScope.launch` で呼び出し、Rust future が drop されたら
 * その Job を cancel する。したがって Room の suspend DAO をそのまま実装に使える
 * （Dispatchers.Default 上で走るのでメインスレッド DB アクセスにもならない）。
 */
class RoomMatchWriteRepository(private val db: ShellDatabase) : MatchWriteRepository {

    // ── read（検証入力の最小セット。これを超える汎用 read 面は注入しない）──

    override suspend fun loadMatch(matchId: UUID): Match = mapRepositoryFailure {
        val row = db.dao().findMatch(matchId.toString())
            ?: throw CoreWriteException.Repository("試合が見つかりません: $matchId")
        row.toDomain()
    }

    override suspend fun loadFactLog(matchId: UUID): List<MatchFact> = mapRepositoryFailure {
        // DAO 側で永続化順に整列している（validators の入力契約）。
        db.dao().factLog(matchId.toString()).map { it.toDomain() }
    }

    override suspend fun loadRosterPlayers(
        homeTeamId: UUID,
        awayTeamId: UUID,
    ): List<PlayerTeamRef> = mapRepositoryFailure {
        db.dao()
            .playersOfTeams(listOf(homeTeamId.toString(), awayTeamId.toString()))
            .map { PlayerTeamRef(playerId = it.id.let(UUID::fromString), teamId = it.teamId.let(UUID::fromString)) }
        // 0 件なら参照整合チェックを skip する後方互換ルールは**コア側**が持つ
        // （roster_context_from_players）。ここでは素直に返すだけ。
    }

    // ── write（素朴 CRUD・検証なし）──

    override suspend fun saveMatch(match: Match) = mapRepositoryFailure {
        db.dao().upsertMatch(match.toRow())
    }

    override suspend fun deleteMatch(matchId: UUID) = mapRepositoryFailure {
        // facts 込みの削除は「ストレージ操作のセマンティクス」なのでシェル側に置く
        // （cascade を trait 実装内に残す判断と同じ — ADR 0005 決定 2）。
        db.withTransaction {
            db.dao().deleteFactsOfMatch(matchId.toString())
            db.dao().deleteMatch(matchId.toString())
        }
    }

    override suspend fun appendFact(matchId: UUID, fact: MatchFact) = mapRepositoryFailure {
        db.dao().upsertFact(fact.toRow(matchId))
    }

    override suspend fun updateFact(matchId: UUID, fact: MatchFact) = mapRepositoryFailure {
        db.dao().upsertFact(fact.toRow(matchId))
    }

    override suspend fun deleteFact(matchId: UUID, factId: UUID) = mapRepositoryFailure {
        db.dao().deleteFact(matchId.toString(), factId.toString())
    }
}

/**
 * チーム / 選手スコープの write repository（6 メソッド: read 2 + write 4）。
 *
 * read は削除の参照整合判定の**材料**（カウント）だけを返す。「使用中だから拒否する」
 * という判断はコアの `record_delete_team` / `record_delete_player` が持つ
 * （ADR 0005 決定 2 — シェル実装側に二重のチェックを置かない）。
 */
class RoomTeamWriteRepository(private val db: ShellDatabase) : TeamWriteRepository {

    // ── read（判定の材料のみ）──

    override suspend fun countMatchesReferencingTeam(teamId: UUID): UInt = mapRepositoryFailure {
        db.dao().countMatchesReferencingTeam(teamId.toString()).toUInt()
    }

    override suspend fun countFactsReferencingPlayer(playerId: UUID): UInt = mapRepositoryFailure {
        db.dao().countFactsReferencingPlayer(playerId.toString()).toUInt()
    }

    // ── write（素朴 CRUD）──

    override suspend fun saveTeam(team: Team) = mapRepositoryFailure {
        db.dao().upsertTeam(team.toRow())
    }

    override suspend fun deleteTeam(teamId: UUID) = mapRepositoryFailure {
        // cascade（所属選手の削除）は判断ではなくストレージ操作のセマンティクスなので
        // ここに残す。1 トランザクションで原子性を保つ（ADR 0005 決定 2）。
        db.withTransaction {
            db.dao().deletePlayersOfTeam(teamId.toString())
            db.dao().deleteTeam(teamId.toString())
        }
    }

    override suspend fun savePlayer(player: Player) = mapRepositoryFailure {
        db.dao().upsertPlayer(player.toRow())
    }

    override suspend fun deletePlayer(playerId: UUID) = mapRepositoryFailure {
        db.dao().deletePlayer(playerId.toString())
    }
}

/**
 * import commit の atomic 発火 repository（1 メソッド）。
 *
 * **全成功 or 1 件も保存しない**が契約。検証はコアが呼ぶ前に済ませてあるので、ここは
 * 検証なしの素朴バッチ。トランザクション境界は DB ハンドルを握るシェルにしか張れない —
 * だからこの 1 本だけ「粗い入口」になっている（ADR 0005 決定 1 の 2026-07-22 追記）。
 *
 * facts はコアが永続化順へ整列済み（`sort_by_persistence_order`）なので、ここで並べ替えない。
 */
class RoomImportWriteRepository(private val db: ShellDatabase) : ImportWriteRepository {

    override suspend fun commitImport(batch: ImportWriteBatch) = mapRepositoryFailure {
        db.withTransaction {
            db.dao().upsertTeams(batch.teams.map { it.toRow() })
            db.dao().upsertPlayers(batch.players.map { it.toRow() })
            db.dao().upsertMatch(batch.match.toRow())
            db.dao().upsertFacts(batch.facts.map { it.toRow(batch.match.id) })
        }
    }
}
