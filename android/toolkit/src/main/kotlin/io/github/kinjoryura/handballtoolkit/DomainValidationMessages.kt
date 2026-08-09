package io.github.kinjoryura.handballtoolkit

import android.content.Context

// (scope, code) → ユーザー向け文言の写像。iOS シムの DomainValidationMessage.swift と
// RecordingErrorPresenter.swift に対応する層で、ADR 0002 決定 3「文言はシェル所有」を
// Android シェルで実体化する。
//
// **コア（Rust）は文言を持たない。** ここが持つのは既定値で、利用側アプリが
// res/values*/strings.xml に同じ name を宣言すれば上書きされる（Android の
// リソースマージはアプリ側が優先）。文言そのものを差し替えたいだけなら、この
// 写像を書き直す必要はない。
//
// 網羅性はコンパイラが担保する: 生成型は sealed なので、コアに case が増えると
// 下の when が非網羅になってビルドが落ちる。ADR 0002 決定 1「文言を書かない限り
// コンパイルが通らない」を Kotlin 側でも成立させている。

/** ユーザーに見せる 1 件分の文言。[body] は「何を直すと先へ進めるか」を含む 1〜2 文。 */
data class DomainValidationMessage(val title: String, val body: String)

/**
 * validation issue のユーザー向け文言。
 *
 * case 名・型名・内部用語（anchor / configuration 等）を UI に漏らさないための単一窓口。
 * 生成型の `toString()` を画面へ流さないこと。
 */
fun DomainValidationIssue.userMessage(context: Context): DomainValidationMessage =
    messageRes().resolve(context)

/**
 * write エラーのユーザー向け文言。
 *
 * [CoreWriteException.ValidationFailed] は運んでいる issue 側の文言へ委譲する
 * （1 件ならそのまま、複数なら件数を前置きして列挙）。それ以外の case が持つ
 * `detail` は**開発者向け診断であって UI に出さない**（ADR 0002 決定 5）。
 */
fun CoreWriteException.userMessage(context: Context): DomainValidationMessage = when (this) {
    is CoreWriteException.ValidationFailed -> validationFailedMessage(context, issues)
    is CoreWriteException.TeamInUse -> MessageRes(
        R.string.handball_toolkit_write_team_in_use_title,
        R.string.handball_toolkit_write_team_in_use_body,
    ).resolve(context)
    is CoreWriteException.PlayerInUse -> MessageRes(
        R.string.handball_toolkit_write_player_in_use_title,
        R.string.handball_toolkit_write_player_in_use_body,
    ).resolve(context)
    is CoreWriteException.Repository -> MessageRes(
        R.string.handball_toolkit_write_repository_title,
        R.string.handball_toolkit_write_repository_body,
    ).resolve(context)
    is CoreWriteException.InsufficientNewIds -> MessageRes(
        R.string.handball_toolkit_write_insufficient_new_ids_title,
        R.string.handball_toolkit_write_insufficient_new_ids_body,
    ).resolve(context)
    is CoreWriteException.MigrationPlanInfeasible -> MessageRes(
        R.string.handball_toolkit_write_migration_plan_infeasible_title,
        R.string.handball_toolkit_write_migration_plan_infeasible_body,
    ).resolve(context)
    is CoreWriteException.ImportDecodeFailed -> MessageRes(
        R.string.handball_toolkit_write_import_decode_failed_title,
        R.string.handball_toolkit_write_import_decode_failed_body,
    ).resolve(context)
}

private fun validationFailedMessage(
    context: Context,
    issues: List<DomainValidationIssue>,
): DomainValidationMessage {
    val messages = issues.map { it.userMessage(context) }
    return when (messages.size) {
        0 -> MessageRes(
            R.string.handball_toolkit_write_validation_failed_title,
            R.string.handball_toolkit_write_validation_failed_body,
        ).resolve(context)
        1 -> messages.single()
        else -> DomainValidationMessage(
            title = context.getString(R.string.handball_toolkit_write_validation_failed_title),
            body = buildString {
                append(
                    context.getString(
                        R.string.handball_toolkit_write_validation_failed_multiple_body,
                        messages.size,
                    ),
                )
                messages.forEach { append('\n').append(it.body) }
            },
        )
    }
}

private class MessageRes(val title: Int, val body: Int) {
    fun resolve(context: Context) =
        DomainValidationMessage(context.getString(title), context.getString(body))
}

private fun DomainValidationIssue.messageRes(): MessageRes = when (this) {
    is DomainValidationIssue.Match -> v1.messageRes()
    is DomainValidationIssue.Configuration -> v1.messageRes()
    is DomainValidationIssue.Fact -> v1.messageRes()
    is DomainValidationIssue.Timeline -> v1.messageRes()
}

private fun MatchValidationError.messageRes(): MessageRes = when (this) {
    is MatchValidationError.SameTeamOnBothSides -> MessageRes(
        R.string.handball_toolkit_match_same_team_on_both_sides_title,
        R.string.handball_toolkit_match_same_team_on_both_sides_body,
    )
    is MatchValidationError.EmptyTitle -> MessageRes(
        R.string.handball_toolkit_match_empty_title_title,
        R.string.handball_toolkit_match_empty_title_body,
    )
    is MatchValidationError.OverlappingRosterSelections -> MessageRes(
        R.string.handball_toolkit_match_overlapping_roster_selections_title,
        R.string.handball_toolkit_match_overlapping_roster_selections_body,
    )
}

private fun ConfigurationValidationError.messageRes(): MessageRes = when (this) {
    is ConfigurationValidationError.NonPositivePhaseDuration -> MessageRes(
        R.string.handball_toolkit_configuration_non_positive_phase_duration_title,
        R.string.handball_toolkit_configuration_non_positive_phase_duration_body,
    )
    is ConfigurationValidationError.EmptyVideoExternalId -> MessageRes(
        R.string.handball_toolkit_configuration_empty_video_external_id_title,
        R.string.handball_toolkit_configuration_empty_video_external_id_body,
    )
}

private fun FactValidationError.messageRes(): MessageRes = when (this) {
    is FactValidationError.NegativeMatchClock -> MessageRes(
        R.string.handball_toolkit_fact_negative_match_clock_title,
        R.string.handball_toolkit_fact_negative_match_clock_body,
    )
    is FactValidationError.NegativeVideoClock -> MessageRes(
        R.string.handball_toolkit_fact_negative_video_clock_title,
        R.string.handball_toolkit_fact_negative_video_clock_body,
    )
    is FactValidationError.NonFiniteMatchClock -> MessageRes(
        R.string.handball_toolkit_fact_non_finite_match_clock_title,
        R.string.handball_toolkit_fact_non_finite_match_clock_body,
    )
    is FactValidationError.NonFiniteVideoClock -> MessageRes(
        R.string.handball_toolkit_fact_non_finite_video_clock_title,
        R.string.handball_toolkit_fact_non_finite_video_clock_body,
    )
    is FactValidationError.InvalidAnchorForConfiguration -> MessageRes(
        R.string.handball_toolkit_fact_invalid_anchor_for_configuration_title,
        R.string.handball_toolkit_fact_invalid_anchor_for_configuration_body,
    )
    is FactValidationError.EmptyTitle -> MessageRes(
        R.string.handball_toolkit_fact_empty_title_title,
        R.string.handball_toolkit_fact_empty_title_body,
    )
    is FactValidationError.EmptyNote -> MessageRes(
        R.string.handball_toolkit_fact_empty_note_title,
        R.string.handball_toolkit_fact_empty_note_body,
    )
    is FactValidationError.DuplicatePrimaryAndRelatedPlayer -> MessageRes(
        R.string.handball_toolkit_fact_duplicate_primary_and_related_player_title,
        R.string.handball_toolkit_fact_duplicate_primary_and_related_player_body,
    )
    is FactValidationError.MissingPlayerForPlayKind -> MessageRes(
        R.string.handball_toolkit_fact_missing_player_for_play_kind_title,
        R.string.handball_toolkit_fact_missing_player_for_play_kind_body,
    )
    is FactValidationError.FreeNoteHasNoContent -> MessageRes(
        R.string.handball_toolkit_fact_free_note_has_no_content_title,
        R.string.handball_toolkit_fact_free_note_has_no_content_body,
    )
    is FactValidationError.PhaseStartMissingEndAnchor -> MessageRes(
        R.string.handball_toolkit_fact_phase_start_missing_end_anchor_title,
        R.string.handball_toolkit_fact_phase_start_missing_end_anchor_body,
    )
    is FactValidationError.PhaseStartAnchorMismatch -> MessageRes(
        R.string.handball_toolkit_fact_phase_start_anchor_mismatch_title,
        R.string.handball_toolkit_fact_phase_start_anchor_mismatch_body,
    )
    is FactValidationError.PhaseStartEndBeforeStart -> MessageRes(
        R.string.handball_toolkit_fact_phase_start_end_before_start_title,
        R.string.handball_toolkit_fact_phase_start_end_before_start_body,
    )
    is FactValidationError.StoppageEndBeforeStart -> MessageRes(
        R.string.handball_toolkit_fact_stoppage_end_before_start_title,
        R.string.handball_toolkit_fact_stoppage_end_before_start_body,
    )
    is FactValidationError.StoppageEndNilInVideoMode -> MessageRes(
        R.string.handball_toolkit_fact_stoppage_end_nil_in_video_mode_title,
        R.string.handball_toolkit_fact_stoppage_end_nil_in_video_mode_body,
    )
    is FactValidationError.StoppageEndPresentInTimerMode -> MessageRes(
        R.string.handball_toolkit_fact_stoppage_end_present_in_timer_mode_title,
        R.string.handball_toolkit_fact_stoppage_end_present_in_timer_mode_body,
    )
    is FactValidationError.TimeoutHasNote -> MessageRes(
        R.string.handball_toolkit_fact_timeout_has_note_title,
        R.string.handball_toolkit_fact_timeout_has_note_body,
    )
    is FactValidationError.EmptyStoppageNote -> MessageRes(
        R.string.handball_toolkit_fact_empty_stoppage_note_title,
        R.string.handball_toolkit_fact_empty_stoppage_note_body,
    )
    is FactValidationError.UnknownTeamReference -> MessageRes(
        R.string.handball_toolkit_fact_unknown_team_reference_title,
        R.string.handball_toolkit_fact_unknown_team_reference_body,
    )
    is FactValidationError.UnknownPlayerReference -> MessageRes(
        R.string.handball_toolkit_fact_unknown_player_reference_title,
        R.string.handball_toolkit_fact_unknown_player_reference_body,
    )
    is FactValidationError.PlayerTeamMismatch -> MessageRes(
        R.string.handball_toolkit_fact_player_team_mismatch_title,
        R.string.handball_toolkit_fact_player_team_mismatch_body,
    )
    is FactValidationError.RelatedPlayerTeamMismatch -> MessageRes(
        R.string.handball_toolkit_fact_related_player_team_mismatch_title,
        R.string.handball_toolkit_fact_related_player_team_mismatch_body,
    )
}

private fun TimelineValidationError.messageRes(): MessageRes = when (this) {
    is TimelineValidationError.TimerWithFactsMissingPhaseStart -> MessageRes(
        R.string.handball_toolkit_timeline_timer_with_facts_missing_phase_start_title,
        R.string.handball_toolkit_timeline_timer_with_facts_missing_phase_start_body,
    )
    is TimelineValidationError.VideoWithFactsMissingPhaseStart -> MessageRes(
        R.string.handball_toolkit_timeline_video_with_facts_missing_phase_start_title,
        R.string.handball_toolkit_timeline_video_with_facts_missing_phase_start_body,
    )
    is TimelineValidationError.VideoHighlightContainsPhaseStart -> MessageRes(
        R.string.handball_toolkit_timeline_video_highlight_contains_phase_start_title,
        R.string.handball_toolkit_timeline_video_highlight_contains_phase_start_body,
    )
    is TimelineValidationError.VideoHighlightContainsStoppage -> MessageRes(
        R.string.handball_toolkit_timeline_video_highlight_contains_stoppage_title,
        R.string.handball_toolkit_timeline_video_highlight_contains_stoppage_body,
    )
    is TimelineValidationError.VideoHighlightMissingTitle -> MessageRes(
        R.string.handball_toolkit_timeline_video_highlight_missing_title_title,
        R.string.handball_toolkit_timeline_video_highlight_missing_title_body,
    )
    is TimelineValidationError.PlayRecordedOutsidePhaseRange -> MessageRes(
        R.string.handball_toolkit_timeline_play_recorded_outside_phase_range_title,
        R.string.handball_toolkit_timeline_play_recorded_outside_phase_range_body,
    )
    is TimelineValidationError.PlayRecordedInsideStoppage -> MessageRes(
        R.string.handball_toolkit_timeline_play_recorded_inside_stoppage_title,
        R.string.handball_toolkit_timeline_play_recorded_inside_stoppage_body,
    )
    is TimelineValidationError.DuplicateShootout -> MessageRes(
        R.string.handball_toolkit_timeline_duplicate_shootout_title,
        R.string.handball_toolkit_timeline_duplicate_shootout_body,
    )
    is TimelineValidationError.ShootoutNotLast -> MessageRes(
        R.string.handball_toolkit_timeline_shootout_not_last_title,
        R.string.handball_toolkit_timeline_shootout_not_last_body,
    )
    is TimelineValidationError.PhaseStartNotContinuousFromPrevious -> MessageRes(
        R.string.handball_toolkit_timeline_phase_start_not_continuous_from_previous_title,
        R.string.handball_toolkit_timeline_phase_start_not_continuous_from_previous_body,
    )
    is TimelineValidationError.StoppagesOverlap -> MessageRes(
        R.string.handball_toolkit_timeline_stoppages_overlap_title,
        R.string.handball_toolkit_timeline_stoppages_overlap_body,
    )
    is TimelineValidationError.StoppageOutsidePhaseRange -> MessageRes(
        R.string.handball_toolkit_timeline_stoppage_outside_phase_range_title,
        R.string.handball_toolkit_timeline_stoppage_outside_phase_range_body,
    )
}
