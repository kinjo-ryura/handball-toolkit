# Error codes

The core never produces user-facing text. Every failure is reported as a **code plus
parameters**, and each shell (iOS, Android, Web, CLI) owns the mapping from code to
localized wording. This document is that mapping's input: it lists every code the
boundary can emit, with its parameters and meaning.

See [ADR 0002](adr/0002-error-model.md) for the reasoning behind this design.

## Two kinds of failure

| Kind | How it reaches you | Where it is listed |
|---|---|---|
| **Validation issues** — values describing why a write was rejected | Returned as a list, never thrown. A non-empty list means the shell must refuse the write | [Validation issues](#validation-issues) |
| **Thrown errors** — the call itself could not complete | Thrown across the FFI boundary (Swift `throws`, Kotlin exception) | [Thrown errors](#thrown-errors) |

Validation is deliberately a *list*: the core reports every problem it finds in one
pass rather than stopping at the first, so a shell can show all of them at once.

## Wire format

A validation issue serializes to a flat object with a `scope`, a `code`, and optional
`params`:

```json
{ "scope": "fact", "code": "negativeMatchClock" }

{ "scope": "fact", "code": "invalidAnchorForConfiguration",
  "params": { "configuration": "timer", "actual": "videoClock", "allowed": ["matchClock"] } }

{ "scope": "timeline", "code": "playRecordedOutsidePhaseRange",
  "params": { "kind": "regular" } }

{ "scope": "match", "code": "overlappingRosterSelections",
  "params": { "playerIds": ["..."] } }
```

- `scope` is one of `match`, `configuration`, `fact`, `timeline`
- **The lookup key for a wording table is the pair `(scope, code)`.** Codes are only
  unique within a scope — `emptyTitle` exists in both `match` and `fact`
- `params` is omitted when the code carries no parameters

### Stability contract

**A published code is a stable contract.** Renaming or removing one is a breaking
change; adding one is not. Shells should therefore handle an unknown code gracefully
(fall back to a generic message) rather than assuming the list is closed.

One code is deliberately irregular: `emptyVideoExternalID` keeps its original casing
(`ID`, not `Id`) because the code names are inherited from the Swift implementation
this core was ported from, and the contract fixes them as-is.

## Validation issues

39 codes across four scopes.

### scope: `match` (3)

Problems with a match's own configuration of teams, title, and rosters.

| code | params | meaning |
|---|---|---|
| `sameTeamOnBothSides` | — | The home and away team are the same team. |
| `emptyTitle` | — | The title is empty, or only whitespace. |
| `overlappingRosterSelections` | `playerIds: [string]` | The same player appears in both teams' roster selections. |

### scope: `configuration` (2)

Problems with the match configuration value itself.

| code | params | meaning |
|---|---|---|
| `nonPositivePhaseDuration` | `seconds: number` | Timer mode requires a phase duration greater than zero. |
| `emptyVideoExternalID` | — | Video and video-highlight modes require a non-empty external video id. |

### scope: `fact` (22)

Problems with a single fact, judged on its own (plus the roster context it references).

**Clock anchors**

| code | params | meaning |
|---|---|---|
| `negativeMatchClock` | — | Match-clock seconds are negative. |
| `negativeVideoClock` | — | Video-clock seconds are negative. |
| `nonFiniteMatchClock` | — | Match-clock seconds are NaN or infinite. |
| `nonFiniteVideoClock` | — | Video-clock seconds are NaN or infinite. |
| `invalidAnchorForConfiguration` | `configuration: string`, `actual: string`, `allowed: [string]` | The anchor kind is not one this configuration accepts (e.g. a video-clock anchor in timer mode). |

`nonFiniteMatchClock` and `nonFiniteVideoClock` have no counterpart in the Swift
original. A negative check of the form `seconds < 0.0` lets NaN through, and a
non-finite value then fails silently downstream — it is written to JSON as `null`,
and every comparison against it is false, so the fact vanishes from per-phase
aggregation without any error. These codes move that failure to write time.

**Text and references**

| code | params | meaning |
|---|---|---|
| `emptyTitle` | — | The title is empty after trimming. |
| `emptyNote` | — | The note is empty after trimming. |
| `duplicatePrimaryAndRelatedPlayer` | — | The primary and related player are the same person. |

**Play facts**

| code | params | meaning |
|---|---|---|
| `missingPlayerForPlayKind` | `kind: string` | This play kind requires a player, and none was given. |
| `freeNoteHasNoContent` | — | A free-note play carries neither note text nor any other content. |

**Phase starts**

| code | params | meaning |
|---|---|---|
| `phaseStartMissingEndAnchor` | — | The phase start has no end anchor. |
| `phaseStartAnchorMismatch` | — | Its start and end anchors are of different kinds. |
| `phaseStartEndBeforeStart` | — | Its end is not strictly after its start (zero-length or reversed). |

**Stoppages**

| code | params | meaning |
|---|---|---|
| `stoppageEndBeforeStart` | — | The stoppage ends before it starts. |
| `stoppageEndNilInVideoMode` | `kind: string` | Video mode requires a closed stoppage interval. |
| `stoppageEndPresentInTimerMode` | `kind: string` | Timer mode requires an open-ended stoppage. |
| `timeoutHasNote` | — | Timeout stoppages must not carry a note. |
| `emptyStoppageNote` | — | The note is present but empty after trimming (use absent instead). |

**Roster integrity**

| code | params | meaning |
|---|---|---|
| `unknownTeamReference` | `teamId: string` | The referenced team is not part of this match. |
| `unknownPlayerReference` | `playerId: string` | The referenced player is not in the roster. |
| `playerTeamMismatch` | `playerId: string`, `teamId: string` | The player does not belong to the referenced team. |
| `relatedPlayerTeamMismatch` | `playerId: string`, `teamId: string` | The related player does not belong to the referenced team. |

### scope: `timeline` (12)

Problems that only exist across the fact log as a whole — ordering, overlap, and
agreement with the configuration. The `R` numbers are the rule identifiers used in
the source comments and in the design documents (`R1`, `R2`, `R4`, `R10` are
historical gaps).

**Configuration agreement**

| code | rule | params | meaning |
|---|---|---|---|
| `timerWithFactsMissingPhaseStart` | R3 | — | A timer-mode match has facts but no phase start. |
| `videoWithFactsMissingPhaseStart` | R5 | — | A video-mode match has facts but no phase start. |
| `videoHighlightContainsPhaseStart` | R6 | — | A highlight has a phase start; highlights have no phase concept. |
| `videoHighlightContainsStoppage` | R9 | — | A highlight has a stoppage; highlights have no stoppage concept. |
| `videoHighlightMissingTitle` | R11 | — | A highlight has no title. Titles distinguish multiple highlights of one match. |

**Recording windows**

| code | rule | params | meaning |
|---|---|---|---|
| `playRecordedOutsidePhaseRange` | R7 | `kind: string \| null` | A play is anchored outside every phase (e.g. during half time). `kind` hints at the adjacent phase, and is null when it cannot be determined. |
| `playRecordedInsideStoppage` | R8 | — | A play is anchored inside a stoppage, i.e. while the match was stopped. |

**Ordering and overlap**

| code | params | meaning |
|---|---|---|
| `duplicateShootout` | — | More than one shootout phase. A match has at most one. |
| `shootoutNotLast` | — | A regular phase starts after the shootout. |
| `phaseStartNotContinuousFromPrevious` | — | In timer mode, a regular phase does not start exactly where the previous one ended. Both gaps and overlaps are rejected, because match-clock seconds accumulate. Video mode is exempt: continuity there is structural. |
| `stoppagesOverlap` | — | Two stoppages overlap. Stopped intervals cannot nest. |
| `stoppageOutsidePhaseRange` | — | A stoppage lies outside every phase. |

## Thrown errors

These are thrown rather than returned, and appear as `throws` in Swift and as
exceptions in Kotlin.

> **Never show the `message` fields to users.** They are diagnostic strings for
> developers, as is the `Display` implementation of these types. Branch on the error
> case and supply your own wording (ADR 0002).

### `CoreWriteError` (7)

Raised by the write entry points.

| case | fields | meaning |
|---|---|---|
| `ValidationFailed` | `issues: [DomainValidationIssue]` | The write was rejected. Nothing was persisted; render the issues. |
| `TeamInUse` | `matchCount: u32` | The team cannot be deleted; it is still referenced by matches. |
| `PlayerInUse` | `factCount: u32` | The player cannot be deleted; facts still reference them. |
| `Repository` | `message: string` | The repository you injected failed. Also carries any exception your implementation threw. |
| `InsufficientNewIds` | `required: usize`, `provided: usize` | Too few pre-generated ids were supplied. Ask for `required`, then retry — this is safe to retry. |
| `MigrationPlanInfeasible` | `message: string` | A video migration could not be planned. A safety net; the wizard's own validation should prevent it. |
| `ImportDecodeFailed` | `message: string` | An import payload could not be decoded into domain types. |

Ids and timestamps are supplied by the shell, never generated by the core, which is
why `InsufficientNewIds` exists. Call the matching `required_*_id_count` function
first, generate that many ids, and pass them in.

### `SampleDtoError` (3)

Raised by the match-data parsing and conversion functions.

| case | fields | meaning |
|---|---|---|
| `InvalidJson` | `message: string` | The JSON does not parse as the expected schema. |
| `Decode` | `error: SampleMatchDecodeErrorV2` | It parsed, but could not be converted to domain types. See below. |
| `InsufficientNewIds` | `required: usize`, `provided: usize` | Too few pre-generated ids. Same contract as above. |

### `SampleMatchDecodeErrorV2` (15)

Carried inside `SampleDtoError.Decode`. Every `Unknown*` case means the document used
a value this version of the core does not recognise — usually a document newer than
the code reading it.

| case | fields | meaning |
|---|---|---|
| `SchemaVersionMismatch` | `found: i64`, `expected: i64` | The document declares a different schema version. |
| `UnknownConfigurationKind` | `string` | Unrecognised configuration kind. |
| `MissingConfigurationPayload` | `string` | The configuration kind requires a payload that is absent. |
| `UnknownPayloadKind` | `string` | Unrecognised fact payload kind. |
| `MissingPayloadBody` | `string` | The payload kind requires a body that is absent. |
| `UnknownPlayKind` | `string` | Unrecognised play event kind. |
| `UnknownControlKind` | `string` | Unrecognised control fact kind. |
| `UnknownStoppageKind` | `string` | Unrecognised stoppage kind. |
| `UnknownPhaseKind` | `string` | Unrecognised phase kind. |
| `UnknownAnchorKind` | `string` | Unrecognised anchor kind. |
| `UnknownVideoProvider` | `string` | Unrecognised video provider. |
| `MissingAnchorBody` | `string` | The anchor kind requires a body that is absent. |
| `UnknownTeamKey` | `string` | A team key that no team in the document defines. |
| `UnknownPlayerKey` | `string` | A player key that no player in the document defines. |
| `MissingPhaseStartEnd` | — | A phase start has no end. |

## Implementing a wording table

1. Key on `(scope, code)` for validation issues, and on the case for thrown errors.
2. Handle unknown codes with a generic fallback — new codes may be added.
3. Do not surface `message` fields or `Display` output.
4. Interpolate `params` into your own sentence rather than concatenating the raw
   values; parameter names are stable, so this is safe.
