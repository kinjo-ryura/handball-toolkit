package com.example.handballshell.db

import java.time.Instant
import java.util.UUID
import uniffi.handball_toolkit.ControlFact
import uniffi.handball_toolkit.FactAnchor
import uniffi.handball_toolkit.Match
import uniffi.handball_toolkit.MatchClock
import uniffi.handball_toolkit.MatchConfiguration
import uniffi.handball_toolkit.MatchFact
import uniffi.handball_toolkit.MatchFactPayload
import uniffi.handball_toolkit.PhaseKind
import uniffi.handball_toolkit.PhaseStartPayload
import uniffi.handball_toolkit.PlayEventKind
import uniffi.handball_toolkit.PlayFact
import uniffi.handball_toolkit.Player
import uniffi.handball_toolkit.PlayerPhoto
import uniffi.handball_toolkit.RosterSelection
import uniffi.handball_toolkit.StoppageKind
import uniffi.handball_toolkit.StoppagePayload
import uniffi.handball_toolkit.Team
import uniffi.handball_toolkit.VideoClock
import uniffi.handball_toolkit.VideoProvider
import uniffi.handball_toolkit.VideoSource

// コアのドメイン型 ↔ Room 行の変換。**シェル側の関心事のみ**で、ドメイン規則は一切持たない
// （持ってはいけない。判断はすべてコア — ADR 0005）。
//
// enum は Kotlin 側の `name`（SCREAMING_SNAKE_CASE）をそのまま保存する。案としては serde の
// camelCase raw value に寄せる手もあるが、その表現は配信 JSON（SAMPLE_DTO_V2）の契約であって
// DB の内部表現ではない。シェル内部で閉じるならバインディングの enum 名が最も安全
// （コア側で case が増減したら enumValueOf が失敗して気づける）。

// ── 共通の小道具 ──

private fun UUID.key(): String = toString()

private fun String.toUuid(): UUID = UUID.fromString(this)

private fun String.toIdList(): List<UUID> =
    if (isEmpty()) emptyList() else split(',').map { it.toUuid() }

private fun List<UUID>.toIdCsv(): String = joinToString(",") { it.key() }

// ── Team / Player ──

fun Team.toRow(): TeamRow = TeamRow(id = id.key(), name = name)

fun TeamRow.toDomain(): Team = Team(id = id.toUuid(), name = name)

fun Player.toRow(): PlayerRow =
    PlayerRow(
        id = id.key(),
        teamId = teamId.key(),
        name = name,
        jerseyNumber = jerseyNumber,
        photoStorageKey = photo?.storageKey,
    )

fun PlayerRow.toDomain(): Player =
    Player(
        id = id.toUuid(),
        teamId = teamId.toUuid(),
        name = name,
        jerseyNumber = jerseyNumber,
        photo = photoStorageKey?.let { PlayerPhoto(storageKey = it) },
    )

// ── Match ──

fun Match.toRow(): MatchRow {
    val kind: String
    var phaseDuration: Double? = null
    var provider: String? = null
    var externalId: String? = null
    when (val config = configuration) {
        is MatchConfiguration.Timer -> {
            kind = "timer"
            phaseDuration = config.phaseDurationSeconds
        }
        is MatchConfiguration.Video -> {
            kind = "video"
            provider = config.v1.provider.name
            externalId = config.v1.externalId
        }
        is MatchConfiguration.VideoHighlight -> {
            kind = "videoHighlight"
            provider = config.v1.provider.name
            externalId = config.v1.externalId
        }
    }
    return MatchRow(
        id = id.key(),
        title = title,
        dateEpochSecond = date.epochSecond,
        dateNano = date.nano,
        homeTeamId = homeTeamId.key(),
        awayTeamId = awayTeamId.key(),
        configurationKind = kind,
        phaseDurationSeconds = phaseDuration,
        videoProvider = provider,
        videoExternalId = externalId,
        benchedPlayerIds = rosterSelection.benchedPlayerIds.toIdCsv(),
        outOfRosterPlayerIds = rosterSelection.outOfRosterPlayerIds.toIdCsv(),
        isHomeOnLeft = isHomeOnLeft,
    )
}

fun MatchRow.toDomain(): Match {
    val source = {
        VideoSource(
            provider = enumValueOf<VideoProvider>(requireNotNull(videoProvider)),
            externalId = requireNotNull(videoExternalId),
        )
    }
    val config = when (configurationKind) {
        "timer" -> MatchConfiguration.Timer(requireNotNull(phaseDurationSeconds))
        "video" -> MatchConfiguration.Video(source())
        "videoHighlight" -> MatchConfiguration.VideoHighlight(source())
        else -> error("未知の configurationKind: $configurationKind")
    }
    return Match(
        id = id.toUuid(),
        title = title,
        date = Instant.ofEpochSecond(dateEpochSecond, dateNano.toLong()),
        homeTeamId = homeTeamId.toUuid(),
        awayTeamId = awayTeamId.toUuid(),
        configuration = config,
        rosterSelection = RosterSelection(
            benchedPlayerIds = benchedPlayerIds.toIdList(),
            outOfRosterPlayerIds = outOfRosterPlayerIds.toIdList(),
        ),
        isHomeOnLeft = isHomeOnLeft,
    )
}

// ── FactAnchor（3 variant を kind + 2 つの秒数列へ平坦化）──

private class AnchorColumns(
    val kind: String,
    val matchSeconds: Double?,
    val videoSeconds: Double?,
)

private fun FactAnchor.columns(): AnchorColumns =
    when (this) {
        is FactAnchor.MatchClock ->
            AnchorColumns("MATCH_CLOCK", v1.elapsedSeconds, null)
        is FactAnchor.VideoClock ->
            AnchorColumns("VIDEO_CLOCK", null, v1.elapsedSeconds)
        is FactAnchor.Both ->
            AnchorColumns("BOTH", matchClock.elapsedSeconds, videoClock.elapsedSeconds)
    }

private fun anchorOf(kind: String, matchSeconds: Double?, videoSeconds: Double?): FactAnchor =
    when (kind) {
        "MATCH_CLOCK" -> FactAnchor.MatchClock(MatchClock(requireNotNull(matchSeconds)))
        "VIDEO_CLOCK" -> FactAnchor.VideoClock(VideoClock(requireNotNull(videoSeconds)))
        "BOTH" -> FactAnchor.Both(
            matchClock = MatchClock(requireNotNull(matchSeconds)),
            videoClock = VideoClock(requireNotNull(videoSeconds)),
        )
        else -> error("未知の anchor kind: $kind")
    }

// ── MatchFact ──

fun MatchFact.toRow(matchId: UUID): FactRow {
    val base = FactRow(
        id = id.key(),
        matchId = matchId.key(),
        recordedAtEpochSecond = recordedAt.epochSecond,
        recordedAtNano = recordedAt.nano,
        payloadKind = "",
        startAnchorKind = "",
        startMatchSeconds = null,
        startVideoSeconds = null,
        endAnchorKind = null,
        endMatchSeconds = null,
        endVideoSeconds = null,
        playKind = null,
        teamId = null,
        playerId = null,
        relatedPlayerId = null,
        title = null,
        phaseKind = null,
        stoppageKind = null,
        note = null,
    )
    return when (val payload = payload) {
        is MatchFactPayload.Play -> {
            val play = payload.v1
            val start = play.anchor.columns()
            base.copy(
                payloadKind = "play",
                startAnchorKind = start.kind,
                startMatchSeconds = start.matchSeconds,
                startVideoSeconds = start.videoSeconds,
                playKind = play.kind.name,
                teamId = play.teamId?.key(),
                playerId = play.playerId?.key(),
                relatedPlayerId = play.relatedPlayerId?.key(),
                title = play.title,
                note = play.note,
            )
        }
        is MatchFactPayload.Control -> when (val control = payload.v1) {
            is ControlFact.PhaseStart -> {
                val start = control.v1.startAnchor.columns()
                val end = control.v1.endAnchor.columns()
                base.copy(
                    payloadKind = "phaseStart",
                    startAnchorKind = start.kind,
                    startMatchSeconds = start.matchSeconds,
                    startVideoSeconds = start.videoSeconds,
                    endAnchorKind = end.kind,
                    endMatchSeconds = end.matchSeconds,
                    endVideoSeconds = end.videoSeconds,
                    phaseKind = control.v1.kind.name,
                )
            }
            is ControlFact.Stoppage -> {
                val start = control.v1.startAnchor.columns()
                val end = control.v1.endAnchor?.columns()
                base.copy(
                    payloadKind = "stoppage",
                    startAnchorKind = start.kind,
                    startMatchSeconds = start.matchSeconds,
                    startVideoSeconds = start.videoSeconds,
                    endAnchorKind = end?.kind,
                    endMatchSeconds = end?.matchSeconds,
                    endVideoSeconds = end?.videoSeconds,
                    stoppageKind = control.v1.kind.name,
                    note = control.v1.note,
                )
            }
        }
    }
}

fun FactRow.toDomain(): MatchFact {
    val startAnchor = anchorOf(startAnchorKind, startMatchSeconds, startVideoSeconds)
    val endAnchor = endAnchorKind?.let { anchorOf(it, endMatchSeconds, endVideoSeconds) }
    val payload = when (payloadKind) {
        "play" -> MatchFactPayload.Play(
            PlayFact(
                kind = enumValueOf<PlayEventKind>(requireNotNull(playKind)),
                teamId = teamId?.toUuid(),
                playerId = playerId?.toUuid(),
                relatedPlayerId = relatedPlayerId?.toUuid(),
                anchor = startAnchor,
                title = title,
                note = note,
            ),
        )
        "phaseStart" -> MatchFactPayload.Control(
            ControlFact.PhaseStart(
                PhaseStartPayload(
                    kind = enumValueOf<PhaseKind>(requireNotNull(phaseKind)),
                    startAnchor = startAnchor,
                    // PhaseStart の endAnchor は「常に値あり」がドメインの不変条件。
                    endAnchor = requireNotNull(endAnchor) { "phaseStart に endAnchor が無い" },
                ),
            ),
        )
        "stoppage" -> MatchFactPayload.Control(
            ControlFact.Stoppage(
                StoppagePayload(
                    kind = enumValueOf<StoppageKind>(requireNotNull(stoppageKind)),
                    startAnchor = startAnchor,
                    endAnchor = endAnchor,
                    note = note,
                ),
            ),
        )
        else -> error("未知の payloadKind: $payloadKind")
    }
    return MatchFact(
        id = id.toUuid(),
        recordedAt = Instant.ofEpochSecond(recordedAtEpochSecond, recordedAtNano.toLong()),
        payload = payload,
    )
}
