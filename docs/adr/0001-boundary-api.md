# 境界 API 目録 — コアが公開する型と関数

## Status

accepted（2026-07-12 起草、同日 grill 済み。handball-project#49「境界 API 目録の作成」）

## 文脈

RecorderDomain（Swift、29 ファイル / 約 2,745 行）を Rust へ移植するにあたり、コアの公開面（境界 API）を先に確定する。この目録は (a) 移植の写経対象の全量定義、(b) 各シェル（iOS / Android / wasm / CLI）との契約、(c) パリティ検証の比較対象の列挙、を兼ねる。

用語は移植元の正準語彙（`apps/HandballRecorder/CONTEXT.md` の Language セクション）に従う。特に: Match clock は**試合通算累積秒**（phase 内秒ではない）、「イベント」ではなく **fact**、「アンカー」という語は型名 `FactAnchor` 以外では使わない（同期点 = sync point と区別）。

## モジュール構成

Swift のディレクトリ構成を 1:1 でミラーする。移植レビューを「Swift ファイル ↔ Rust ファイル」の対応で行えるようにするため。

| Rust モジュール | Swift 対応 | 内容 |
|---|---|---|
| `ids` | `Identifiers.swift` | `MatchId` / `TeamId` / `PlayerId` / `FactId` |
| `clock` | `Clock/` | `MatchClock` / `VideoClock` / `FactAnchor` / `FactAnchorKind` |
| `configuration` | `Configuration/` | `MatchConfiguration` / `MatchConfigurationKind` / `PhaseKind` / `VideoSource` / `VideoProvider` / `CaptureMethod`（`ContentKind` は移植しない — 後述） |
| `entities` | `Entities/` | `Match` / `Team` / `Player` / `PlayerPhoto` / `RosterSelection` |
| `facts` | `Facts/` | `MatchFact` / `MatchFactPayload` / `PlayFact` / `PlayEventKind` / `ControlFact` / `PhaseStartPayload` / `StoppagePayload` / `StoppageKind` |
| `projection::time_segment` | `Projection/TimeSegment.swift` | `TimeSegment` |
| `projection::segment_resolver` | `Projection/SegmentResolver.swift` | `SegmentResolver` |
| `projection::timeline` | `Projection/TimelineProjection.swift` | `TimelineProjection` / `ResolvedFact` |
| `projection::summary` | `Projection/SummaryProjection.swift` | `SummaryProjection` ほか |
| `projection::score_progression` | `Projection/ScoreProgressionProjection.swift` | `ScoreProgressionProjection` ほか |
| `projection::live_match` | `Projection/LiveMatchProjection.swift` | `LiveMatchProjection` / `MatchTimerState` / `AvailableActions` |
| `validation` | `Validation/` | エラー型 4 系統 + `DomainValidationIssue`（文言レイヤは移植しない → ADR 0002） |
| `validators` | `Validators/` | `fact_validator` / `fact_log_validator` / `match_write_validator` / `configuration_validator` / `match_validator` |
| `sample_dto` | （アプリ層 `SampleMatches/V2/` 由来） | Sample DTO V2 の serde 型 + domain 変換（→ ADR 0003） |

## 型目録（Swift → Rust）

命名規約: 型名は Swift を踏襲（`ID` → `Id` のみ Rust 慣習に合わせる）。enum variant は Rust の PascalCase とし、serde で Swift の raw value（camelCase 文字列）へ rename する。

### ids

| Swift | Rust | 備考 |
|---|---|---|
| `MatchID` / `TeamID` / `PlayerID` / `FactID`（= UUID の typealias） | `pub type MatchId = Uuid;` ほか type alias | Swift 同様 type alias で忠実移植（grill 確定 2026-07-12）。newtype 化（型安全強化）はパリティ完走後の別タスクとして Issue 化 |

### clock

| Swift | Rust | 備考 |
|---|---|---|
| `MatchClock { elapsedSeconds: TimeInterval }` | `MatchClock { elapsed_seconds: f64 }` | 試合通算累積秒。Stoppage 中・phase 境界では進まない。shootout は開始時点で固定 |
| `VideoClock { elapsedSeconds }` | `VideoClock { elapsed_seconds: f64 }` | 動画再生位置 |
| `FactAnchor`（enum: `.matchClock` / `.videoClock` / `.both(match:video:)`） | `enum FactAnchor { MatchClock(MatchClock), VideoClock(VideoClock), Both { match_clock, video_clock } }` | 計算プロパティ `kind()` / `match_clock()` / `video_clock()` / `match_elapsed_seconds()` / `video_elapsed_seconds()` をメソッドで移植 |
| `FactAnchorKind`（String raw） | `enum FactAnchorKind` + serde rename | `matchClock` / `videoClock` / `both` |

### configuration

| Swift | Rust | 備考 |
|---|---|---|
| `MatchConfiguration`（sum type: `.timer(phaseDurationSeconds:)` / `.video(VideoSource)` / `.videoHighlight(VideoSource)`） | `enum MatchConfiguration { Timer { phase_duration_seconds: f64 }, Video(VideoSource), VideoHighlight(VideoSource) }` | illegal state を型で排除する設計をそのまま維持 |
| helper: `kind` / `captureMethod` / `videoSource` / `phaseDurationSeconds` | 同名メソッド | `contentKind` / `ContentKind` は**移植しない**（grill 確定 2026-07-12）。CONTEXT.md の「避ける」語であり、かつドメイン内部で未使用（利用者はシェル UI のみ）。必要なシェルは case の switch で自前導出する |
| `PhaseKind`（`.regular` / `.shootout`） | `enum PhaseKind { Regular, Shootout }` | phase 番号は出現順導出、役割名 enum は持たない |
| `VideoSource { provider, externalID }` / `VideoProvider`（`.youtube` / `.local`） | 同構造 | `.local` は端末固有 PHAsset 参照。コアは中身を解釈しない |

### entities

| Swift | Rust | 備考 |
|---|---|---|
| `Match { id, title?, date, homeTeamID, awayTeamID, configuration, rosterSelection, isHomeOnLeft }` | 同構造（`title: Option<String>`, `date: DateTime<Utc>`） | `isHomeOnLeft` は表示設定だが Match に永続。忠実移植 |
| `RosterSelection { benchedPlayerIDs: Set, outOfRosterPlayerIDs: Set }` | `BTreeSet<PlayerId>` × 2 | HashSet でなく BTreeSet を採用（エラー payload・ゴールデン出力の決定性。集合演算の意味論は同一） |
| `Team { id, name }` / `Player { id, teamID, name, jerseyNumber?, photo? }` / `PlayerPhoto { storageKey }` | 同構造 | photo は storage 参照キーのみ（実体はシェル管理） |

### facts

| Swift | Rust | 備考 |
|---|---|---|
| `MatchFact { id, recordedAt: Date, payload }` | `MatchFact { id: FactId, recorded_at: DateTime<Utc>, payload: MatchFactPayload }` | id / timestamp はここに一元化。`recorded_at` は**整列 tie-break 専用**（位置づけは anchor） |
| `MatchFactPayload`（`.play` / `.control`） | `enum MatchFactPayload { Play(PlayFact), Control(ControlFact) }` | helper `anchor()`（Play は唯一の anchor、Control は startAnchor） |
| `PlayFact { kind, teamID?, playerID?, relatedPlayerID?, anchor, title?, note? }` | 同構造 | phase フィールドは持たない（PhaseStart range から逆引き）。teamID は全 kind optional（goal / shotMissed は UI が実質必須担保） |
| `PlayEventKind`（6 種） | `Goal / ShotMissed / FreeNote / YellowCard / TwoMinuteSuspension / RedCard` | serde rename で camelCase |
| `ControlFact`（`.phaseStart(PhaseStartPayload)` / `.stoppage(StoppagePayload)`） | 同構造 + helper `start_anchor()` / `end_anchor()` | 旧 6 種 flat enum の置換。`phaseEnded` は存在しない（end は PhaseStart の range に統合） |
| `PhaseStartPayload { kind, startAnchor, endAnchor }` | 同構造 | **endAnchor は non-optional**（生成時に必ず入力を経る不変条件） |
| `StoppagePayload { kind, startAnchor, endAnchor?, note? }` | 同構造 | timer モードは end nil / video モードは end 必須（validator が担保） |
| `StoppageKind`（`.timeout` / `.pause`） | `Timeout / Pause` | 2 種で確定、細分化しない（CONTEXT.md） |

### projection（出力型）

| Swift | Rust | 備考 |
|---|---|---|
| `TimeSegment { kind(running/stopped), phaseKind?, matchElapsedStart, matchElapsedEnd?, videoElapsedStart?, videoElapsedEnd?, startFactID?, endFactID?, stoppageKind? }` | 同構造 | メソッド: `match_elapsed_duration` / `video_elapsed_duration` / `contains_video_elapsed` / `contains_match_elapsed` / `match_elapsed_for_video_elapsed` / `video_elapsed_for_match_elapsed` |
| `SegmentResolver { segments, phases }` + nested `Phase { factID, kind, matchElapsedStart?, matchElapsedEnd?, videoElapsedStart?, videoElapsedEnd? }` | 同構造 | 中核。セマンティクス保存リスト（後述）参照 |
| `TimelineProjection { resolvedFacts, resolver }` / `ResolvedFact { fact, resolvedMatchClock?, resolvedVideoClock? }` | 同構造 | |
| `SummaryProjection { homeScore, awayScore, homeTeam, awayTeam, playerStats, phaseSummaries }` + `TeamSummaryLine` / `PlayerStatLine` / `PhaseSummaryLine` | 同構造 | 導出プロパティ（`shotAttempts` / `scoringRate` 等）はメソッド化 |
| `ScoreProgressionProjection { points, phaseSpans, totalSeconds, maxAbsDiff }` + `ScoreProgressionPoint` / `ScoreProgressionPhaseSpan` | 同構造 | step-doubling 済み系列（各 goal 時刻に前後 2 点） |
| `LiveMatchProjection { currentPhaseKind?, currentPhaseIndex?, timerState, currentMatchClock?, availableActions }` + `MatchTimerState`（6 状態）/ `AvailableActions`（bool 6 個） | 同構造 | video モード専用 build のみ（timer モードの live はシェル側） |

### 準拠プロトコルの対応

- `Codable` → serde `Serialize` / `Deserialize`。**domain 型の serde 形式は Swift Codable 合成形式との互換を要求しない**（Swift 側で Codable は fixture / debug 用途と明記されており永続化契約ではない。コーパス互換が必要なのは `sample_dto` のみ → ADR 0003）
- `Equatable` / `Hashable` → `PartialEq` は全型 derive。`Eq` / `Hash` は f64 を含む型（clock 系・TimeSegment 等）では derive できないため、**使用実態ベースで最小化**（Swift の Hashable 全付与は慣習であり、コアのロジックが要求するのは `Set<FactAnchorKind>` 等の enum のみ）
- `Sendable` → 純粋値型のため `Send + Sync` は自明

## 関数目録

すべて stateless な関連関数（Swift の `static func` に対応）。入力は借用（`&`）、出力は所有値。

### projections

```rust
impl SegmentResolver {
    pub fn build(facts: &[MatchFact]) -> SegmentResolver;
    pub fn resolve_match_clock(&self, video: VideoClock) -> Option<MatchClock>;
    pub fn resolve_video_clock(&self, match_clock: MatchClock) -> Option<VideoClock>;
    pub fn phase_kind(&self, match_elapsed_seconds: f64) -> Option<PhaseKind>;
    pub fn phase_index(&self, match_elapsed_seconds: f64) -> Option<usize>;   // regular のみカウント
    pub fn segment_for_video_elapsed(&self, seconds: f64) -> Option<&TimeSegment>;
    pub fn segment_for_match_elapsed(&self, seconds: f64) -> Option<&TimeSegment>;
    pub fn phase_for_match_elapsed(&self, seconds: f64) -> Option<&Phase>;
}

impl TimelineProjection {
    pub fn build(match_: &Match, facts: &[MatchFact]) -> TimelineProjection;
    pub fn resolved_fact(&self, id: FactId) -> Option<&ResolvedFact>;
}

impl SummaryProjection {
    pub fn build(match_: &Match, facts: &[MatchFact]) -> SummaryProjection;              // phaseSummaries 空
    pub fn build_with_timeline(match_: &Match, timeline: &TimelineProjection) -> SummaryProjection;
}

impl ScoreProgressionProjection {
    pub fn build(match_: &Match, facts: &[MatchFact]) -> Option<ScoreProgressionProjection>;
    pub fn build_with_timeline(match_: &Match, timeline: &TimelineProjection) -> Option<ScoreProgressionProjection>;
}

impl LiveMatchProjection {
    pub fn build_video_mode(
        match_: &Match,
        timeline: &TimelineProjection,
        current_video_clock: Option<VideoClock>,
    ) -> LiveMatchProjection;
}
```

- Swift の「facts 版 / timeline 版」二系統オーバーロード（resolver を二度作らないための API パターン）は、Rust ではオーバーロード不可のため `build` / `build_with_timeline` の命名で維持する。

### validators

```rust
pub fn validate_match(match_: &Match) -> Vec<DomainValidationIssue>;
pub fn validate_configuration(config: &MatchConfiguration) -> Vec<DomainValidationIssue>;

pub struct RosterContext {
    pub home_team_id: TeamId,
    pub away_team_id: TeamId,
    pub player_team_lookup: BTreeMap<PlayerId, TeamId>,
    pub known_player_ids: Option<BTreeSet<PlayerId>>,   // Some で dangling 検出 on
}
impl RosterContext { pub fn empty(home: TeamId, away: TeamId) -> RosterContext; }

pub fn validate_match_fact(fact: &MatchFact, config: &MatchConfiguration, roster: &RosterContext) -> Vec<DomainValidationIssue>;
pub fn validate_play_fact(fact: &PlayFact, config: &MatchConfiguration, roster: &RosterContext) -> Vec<DomainValidationIssue>;
pub fn validate_control_fact(fact: &ControlFact, config: &MatchConfiguration) -> Vec<DomainValidationIssue>;

pub fn validate_fact_log(facts: &[MatchFact], match_: &Match) -> Vec<DomainValidationIssue>;

// 永続化直前の集約窓口（非空を返したらシェルは書き込みを拒否する契約）
pub fn validate_append(fact: &MatchFact, existing_facts: &[MatchFact], match_: &Match, roster: Option<&RosterContext>) -> Vec<DomainValidationIssue>;
pub fn validate_update(fact: &MatchFact, existing_facts: &[MatchFact], match_: &Match, roster: Option<&RosterContext>) -> Vec<DomainValidationIssue>;
pub fn validate_delete(removed_fact_id: FactId, existing_facts: &[MatchFact], match_: &Match) -> Vec<DomainValidationIssue>;
```

- Swift の名前空間 enum（`FactValidator` 等）は Rust ではモジュール + 自由関数で表現する。
- `validate_delete` が per-fact 検査を走らせない（whole-log のみ）非対称性は仕様（削除は「消える 1 件」のため）。

### 入力契約（precondition）

- `facts` は**永続化順（累積秒 → recordedAt → id）でソート済み**である前提（Swift `FactLogValidator` と同一の契約）。SegmentResolver は PhaseStart を内部で primary 秒ソートするが、log 全体の防御的ソートはコアでは行わない。
- timestamp / ID はシェルが発行して fact に載せて渡す。コアは `now()` / UUID 生成を持たない（決定性）。

## 型マッピング方針（Foundation → Rust）

| Foundation | Rust | 注意 |
|---|---|---|
| `UUID` | `uuid::Uuid` | 決定的ソートキー用途（Swift `uuidString` 昇順）は `Uuid` の `Ord`（バイト順 = hex 文字列順と同順）で置換。パリティ検証で同順性を確認する |
| `Date` | `chrono::DateTime<Utc>` | 用途は順序比較（tie-break）と ISO 8601 serde のみ。書式化・TZ 計算はコアに無い。crate は chrono で確定（grill 2026-07-12。用途が狭く最も枯れた選択肢を採る。time / jiff は差が出ない） |
| `TimeInterval` | `f64` | 実体は Double の別名。等価境界比較（`start == end` の degenerate 判定等）は移植で演算順を変えないこと。ソートは `total_cmp` で決定化 |
| `Set<UUID>` | `BTreeSet<Uuid>` | 決定性優先（前述） |
| `String.trimmingCharacters(.whitespacesAndNewlines)` | `str::trim()` | どちらも Unicode 空白基準でほぼ同等。厳密同等性はパリティ検証の確認項目 |
| `Int` | `i64` | Swift Int は 64-bit（Apple プラットフォーム）。該当は `Player.jerseyNumber` 等の値フィールド。コレクションの index を返す API のみ Rust 慣習の `usize`（関数目録 `phase_index` 参照） |

依存 crate は `uuid` / `serde` / `serde_json` / 日時 crate の最小集合に留める（wasm 対応を壊さない）。

## 保存すべきセマンティクス（変えてはいけないリスト）

移植で「改善」してはならない挙動。パリティ検証（ADR 0003）とテスト移植の重点対象。

1. **baseline rolling forward** — 各 phase の matchClock 起点は anchor 明示なら override、なければ前 phase の end を継承。計算順は videoClock 順（記録順ではない）
2. **matchEnd 導出の 4 分岐** — ① endAnchor.matchClock 明示 → 採用 ② shootout → matchStart に固定 ③ video モード → running 区間の実時間累積 ④ フォールバック matchStart
3. **shootout の degenerate clock** — matchClock は phase 全体で固定・videoClock のみ進行。video→match は解決可、match→video は逆引き不可（非対称）
4. **stoppage carve** — video 軸で running / stopped を分割。stopped は match 軸で据え置き。start/end は phase 範囲に clamp
5. **半開区間 `[start, end)` + degenerate 特例** — `start == end` のときのみ単一点一致を許す。phase 境界は後の phase に属す（出現順で最初にヒット）
6. **`resolve_video_clock` の running 優先** — 同一 matchClock が複数 video 位置に対応するため running segment を優先 lookup
7. **SummaryProjection の解決不能 goal 黙殺** — matchClock を解決できない goal は phase 別集計から黙って除外（header 合計 ≥ Σphase）。正常データでは R7 が防ぐ safety net
8. **ScoreProgression の step-doubling** — 各 goal 時刻に前後 2 点、先頭 (0,0)・末尾 (total, 最終)、`diff = away - home`、`maxAbsDiff = max(1, …)`、regular phase 無し / goal 0 件は None
9. **playerStats の決定的ソート** — Swift `uuidString` 昇順相当

## 将来の境界拡張候補（移植完了後）

移植期間中の境界はこの目録で凍結する（忠実移植）。完了後の拡張は「**プラットフォーム API を使わない純粋ロジックはコアへ沈め、シェルの二重実装を減らす**」を判断ルールとする（2026-07-12 設計討議）。現時点の候補:

- **タイマーモード phase 自動生成（D-snap auto-create）** — 現在 `RecorderApplication` の `RecordingScreenStore.ensureTimerPhasesCovering`（約 35 行）にある「どの D-snap phase が欠けていて、それぞれ何秒〜何秒で作るべきか」の計算。入力（fact 列 + phase duration + 記録秒）→ 出力（作るべき PhaseStart のリスト）の純粋関数で、Android シェル実装時に二重実装になる筆頭候補。`missing_timer_phases(facts, config, seconds) -> Vec<PhaseStartPayload>` のような形で境界に追加する。挙動の正典は HandballRecorder の ADR 0001「タイマーモードの記録は phase を確認なしで auto-create する」（区間 index の丸め方向の混在に注意）

## Considered options

- **モジュール構成を Rust 流に再編**（例: validation と validators の統合、projection のフラット化）→ 却下。移植期間中は 1:1 ミラーが差分レビューとパリティ検証の追跡性で勝る。再編は移植完了後の改善候補
- **ID の newtype 化**（`struct MatchId(Uuid)`）→ 却下（grill 確定 2026-07-12）。Swift 側も型安全性は無く、移植期間中は「形を変えない」ことが写経ミス検出の武器になるため type alias を採る。newtype 化はパリティ完走後の型安全化タスクとして Issue 化（backlog）
- **API を「JSON in → JSON out」の文字列境界にする** → 却下。Rust ネイティブ利用（CLI / テスト）で型を失う。serde 層は境界の外側（各バインディング）に置く
- **保存コールバック注入**（シェルの save 関数をコアの `add()` に渡し、validate → save の調停をコアが担う案）→ 却下（2026-07-12 設計討議）。理由: (1) DB ハンドルがシェルにある以上「全書き込みが検証を通る」保証は結局シェル側の規約であり、強制力は現行の repository 内包方式と同質 (2) シェルから消えるのは guard 数行だけで、コールバックの trait 化・actor 境界の往復・エラー写像などの梱包材が 10 行強純増し「シェルコード削減」の目的に逆行 (3) async コールバック FFI の問題系一式（隔離・寿命・キャンセル・再入）を最も頻繁に通る書き込み経路で踏む。**再検討トリガー: Android シェル実装時に validate → save 中間役の二重実装が実際に痛いと体感したとき**。書き込み経路を repository 一本に集約し続ける限り、この移行は追加的で手戻りは小さい
- **write-plan パターン**（`plan_append` が検証合格時のみ「書き込み券（WritePlan）」を返し、シェルの save 関数が WritePlan しか受け取らない形にして validate 忘れを型で防ぐ案）→ 今回は見送り（grill 確定 2026-07-12）。移植元 Swift に無い API を移植期間中に新設しない。既存 `validate_append` を包むだけの追加品なので、後から足しても既存境界を壊さない。再検討トリガーは保存コールバック注入と同じ（Android シェルで validate → save 中間役の痛みを体感したとき）で、**その時点で write-plan / コールバック注入のどちらを採るか比較して決める**

## Consequences

- コアの公開面が確定し、移植は「この目録を満たす」作業として進捗測定できる
- Swift 側の型変更（今後の HandballRecorder 開発）はこの目録との差分管理が必要になる（移植完了までは Swift が正、完了後の二重管理方針は別途）
- 文言レイヤ（`DomainValidationMessage`）を境界の外に出す再設計は ADR 0002 で定義

## 参照

- 移植元: `apps/HandballRecorder/Packages/RecorderDomain/Sources/RecorderDomain/`
- 型仕様: `apps/HandballRecorder/docs/redesign/DOMAIN_TYPES_V1.md`
- 語彙: `apps/HandballRecorder/CONTEXT.md`（Language セクション）
- 背景: `handball-project/docs/research/handballrecorder-rust-core.md` / handball-project#49
