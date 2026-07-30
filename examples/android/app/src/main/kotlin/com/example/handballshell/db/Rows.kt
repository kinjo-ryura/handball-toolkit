package com.example.handballshell.db

import androidx.room.Entity
import androidx.room.Index
import androidx.room.PrimaryKey

// Room の行表現。**列単位の手書きマッピング**にしているのは、コアのドメイン型（sum type を
// 含む）をシェルがどう平坦化するかがシェル契約の一部であり、サンプルとしてそこが見えて
// いてほしいため（JSON blob 1 列に押し込むと隠れてしまう）。iOS 側の SwiftData 実装も
// 同じく列単位の手書き Mapper。
//
// ID は小文字 UUID 文字列（java.util.UUID.toString() の既定）。**この選択は読み出し順に
// 効く** — 永続化順の最終 tie-break は FactId の Uuid バイト順であり、小文字 hex の
// TEXT 昇順はバイト昇順と一致する。大文字と混在させると壊れる。

@Entity(tableName = "team")
data class TeamRow(
    @PrimaryKey val id: String,
    val name: String,
)

@Entity(
    tableName = "player",
    indices = [Index("teamId")],
)
data class PlayerRow(
    @PrimaryKey val id: String,
    val teamId: String,
    val name: String,
    val jerseyNumber: Long?,
    /** PlayerPhoto は storage への参照だけ。実ファイルの lifecycle はシェルの責務。 */
    val photoStorageKey: String?,
)

@Entity(tableName = "match")
data class MatchRow(
    @PrimaryKey val id: String,
    val title: String?,
    // java.time.Instant を無損失で往復させるため 2 列に分ける。epoch millis 1 列だと
    // ナノ秒が落ちる — recordedAt は整列の tie-break キーなので精度を落としたくない。
    val dateEpochSecond: Long,
    val dateNano: Int,
    val homeTeamId: String,
    val awayTeamId: String,
    // ── MatchConfiguration（sum type を discriminator + 列で平坦化）──
    /** "timer" | "video" | "videoHighlight" */
    val configurationKind: String,
    /** timer のみ */
    val phaseDurationSeconds: Double?,
    /** video / videoHighlight のみ */
    val videoProvider: String?,
    val videoExternalId: String?,
    // ── RosterSelection（サンプルなのでカンマ区切りの単純表現）──
    val benchedPlayerIds: String,
    val outOfRosterPlayerIds: String,
    val isHomeOnLeft: Boolean,
)

@Entity(
    tableName = "fact",
    indices = [Index("matchId")],
)
data class FactRow(
    @PrimaryKey val id: String,
    val matchId: String,
    val recordedAtEpochSecond: Long,
    val recordedAtNano: Int,
    /** "play" | "phaseStart" | "stoppage" */
    val payloadKind: String,
    // ── 代表 anchor（play は anchor、control は startAnchor）──
    /** "matchClock" | "videoClock" | "both" */
    val startAnchorKind: String,
    val startMatchSeconds: Double?,
    val startVideoSeconds: Double?,
    // ── 終了 anchor（phaseStart は必須 / stoppage は任意 / play は常に null）──
    val endAnchorKind: String?,
    val endMatchSeconds: Double?,
    val endVideoSeconds: Double?,
    // ── play 固有 ──
    val playKind: String?,
    val teamId: String?,
    val playerId: String?,
    val relatedPlayerId: String?,
    val title: String?,
    // ── control 固有 ──
    val phaseKind: String?,
    val stoppageKind: String?,
    // ── play / stoppage 共通 ──
    val note: String?,
)
