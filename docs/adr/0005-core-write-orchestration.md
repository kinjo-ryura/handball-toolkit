# 保存・更新発火のコア移管 — repository の FFI 注入と write orchestration

## Status

accepted（2026-07-18 起草、2026-07-19 grill 済み。handball-project#61「保存・更新の発火を Rust コアへ移す」）

## 文脈

#56（ADR 0004 実装順序 1–5）で、ドメイン型・投影・検証・サンプル DTO の変換/エンコードは Rust コアへ移った。しかし**永続化の発火は今も Swift 側**にある: store / importer / view が「いつ何を保存するか」を判断し、repository（SwiftData 実装）を直接呼ぶ。本 ADR は、シェルが repository 実装（保存・更新関数）を FFI 越しにコアへ注入し、**コアが「検証 → 発火」を一手に持つ**境界への移行を定める。

### ADR 0001 の却下との関係（本 ADR が覆すもの）

保存コールバック注入は ADR 0001（2026-07-12）で一度却下されている。却下理由と再検討条件:

1. DB ハンドルがシェルにある以上「全書き込みが検証を通る」保証は結局シェル規約で、repository 内包方式と強制力が同質
2. シェルから消えるのは guard 数行だけで、trait 化・actor 境界往復・エラー写像の梱包材が純増し「シェルコード削減」に逆行
3. async コールバック FFI の問題系一式（隔離・寿命・キャンセル・再入）を最頻の書き込み経路で踏む
- 再検討トリガー: **Android シェル実装時**に validate → save 中間役の二重実装が実際に痛いと体感したとき。その時点で write-plan パターンと比較して決める

トリガー（Android の痛み）は未発火だが、#61 の判断（2026-07-18）で前倒しする。却下時から前提が 3 点変わったため:

- **(a) 中間役はすでに実在する**: ADR 0004 で validators が FFI 公開され、`SwiftDataMatchRepository` は `context.save()` 直前に Rust の `validateAppend/Update/Delete` を呼ぶ形になった（棚卸し 2026-07-18）。「検証 → 発火」の調停コードは既に書かれており、問題はその**所在が Swift 実装層に埋まっている**こと。移すものは guard 数行ではなく、この調停そのもの + 後述の発火判断群
- **(b) 消せる Swift は guard 数行ではない**: 棚卸しで、シェル最厚の発火判断は `RecordingScreenStore.ensureTimerPhasesCovering`（タイマーモードの phase 自動補完 — D-snap index 算出 + 被覆差分 + 連鎖 append）と `MigrateToVideoStore.commit`（config 先行 save → fact 逐次 update の順序設計）と判明。これらはドメイン規則を含む発火 orchestration であり、Android でも同じものを書くことになる
- **(c) 機構リスクがスパイクで既知になった**: uniffi 0.32 の async foreign trait は下記スパイクで生成まで確認。問題系（隔離・寿命・キャンセル・再入）は「未知のリスク」から「設計で個別に潰す既知の制約」に変わった

### スパイク検証（2026-07-18、uniffi 0.32）

`#[uniffi::export(with_foreign)]` + `#[async_trait::async_trait]` の foreign trait と、それを消費する async export 関数で確認（検証コードは破棄済み。再現は本 ADR の記述で足りる — ADR 0004 と同じ扱い）:

| 検証項目 | 結果 |
|---|---|
| async foreign trait の生成 | `public protocol SpikeMatchWriteRepository: AnyObject, Sendable { func appendFact(matchId: Uuid, fact: MatchFact) async throws }` — Swift 実装をそのまま準拠で渡せる |
| async export 関数 | `public func spikeAppendValidated(repo:, match:, existingFacts:, fact:) async throws` — Rust future は Swift 側 executor が poll（Rust ランタイム不要、現構成不変） |
| エラー伝播 | trait メソッドは `Result<_, E>` 必須。`E: uniffi::Error` が Swift `throws` に写る |
| callback 側の panic / 未知エラー | `From<uniffi::UnexpectedUniFFICallbackError>` を error 型に実装して構造化エラーへ畳む。**未実装だと Rust panic → panic=abort 構成ではアプリクラッシュ**のため実装必須 |
| 制約 | trait は `Send + Sync (+ Debug)` 境界・全引数は値渡し。キャンセルは uniffi 非対応 |
| 依存追加 | `async-trait`（feature `uniffi` 配下の optional）のみ。wasm / CLI ビルドには入らない |

### 棚卸し要約（2026-07-18、詳細は #61 コメント）

- 書き込み呼び出しサイトは store 群（RecorderApplication）・importer / migrator・view（`EditMatchViewV2.save` が repository 直呼び）に散在
- **「保存してよいか」の最終判断はすでに Rust**: `SwiftDataMatchRepository` の fact 3 経路（append/update/delete）が保存直前に `validate*` を呼び、違反は `RepositoryError.validationFailed` で保存阻止
- シェル最厚の発火判断: ① `ensureTimerPhasesCovering`（phase 自動補完の二段 append）② `MigrateToVideoStore.commit`（anchor 変換 + 保存順序設計）③ repository 実装内の validate → save 調停
- team/player の CRUD は 1〜2 行の薄い転送（参照整合 `teamInUse` / `playerInUse` は SwiftData クエリと密結合）

## 決定

### 1. 境界形状 — 「判断・計画は純粋関数、発火は注入 repository を await する orchestration」

2 層に分けてコアに置く:

- **計画層（純粋関数・feature 非依存）**: 「この操作で何をどの順に保存すべきか」を fact 列 in → 書き込み計画 out で算出する純粋関数群。wasm / CLI / テストから直接使え、設計不変条件（stateless・決定性・粗い境界）を満たす。ADR 0001 が見送った write-plan パターンをコア**内部**表現として採用する
- **発火層（feature `uniffi` 配下の async orchestration）**: foreign trait `MatchWriteRepository` を `Arc<dyn>` で受け、**読む → 検証 → 発火**を一続きで実行する薄い export 関数群。validation 違反は発火せず構造化エラーで拒否する

ADR 0001 の宿題「write-plan / コールバック注入のどちらか」への回答は**両取り**: 判断の実体は write-plan（純粋・共有可能）、境界の形はコールバック注入（発火ループ・順序設計までコアが所有し、Android と共有できる）。

**検証入力は保存瞬間の DB 真実（grill 確定 2026-07-19）**: 現行の SwiftData 実装は保存直前に fresh `ModelContext` で match / fact 列 / roster を DB から読み直して validation に掛けており、「門番は常に最新の真実で判定する」性質を持つ。これを保つため、foreign trait には write 経路の検証入力に必要な**最小 read セット**（`load_match` / `load_fact_log` / roster 用の選手一覧）を含め、コアが read → validate → fire を完結する。シェルが手元の読み込み済み fact 列を引数で渡す形は、store のコピーが古い場合（Mac マルチウィンドウ等）に現行より検証が緩くなるセマンティクス変更のため採らない。roster の「選手 0 人なら参照整合チェックを skip」の後方互換ルールは、Swift 実装からコア側へ移す（判断はコアへ）。

trait は既存 repository の分割を写して 2 本（`MatchWriteRepository` / `TeamWriteRepository`）とする:

```rust
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait MatchWriteRepository: Send + Sync + std::fmt::Debug {
    // read（write 経路の検証入力に限る最小セット）
    async fn load_match(&self, match_id: Uuid) -> Result<Match, CoreWriteError>;
    async fn load_fact_log(&self, match_id: Uuid) -> Result<Vec<MatchFact>, CoreWriteError>;
    /// home / away 両チームの (player_id, team_id) 一覧（roster 構築材料）。
    async fn load_roster_players(
        &self, home_team_id: Uuid, away_team_id: Uuid,
    ) -> Result<Vec<PlayerTeamRef>, CoreWriteError>;
    // write（素朴 CRUD、検証なし）
    async fn save_match(&self, match_: Match) -> Result<(), CoreWriteError>;
    async fn delete_match(&self, match_id: Uuid) -> Result<(), CoreWriteError>;
    async fn append_fact(&self, match_id: Uuid, fact: MatchFact) -> Result<(), CoreWriteError>;
    async fn update_fact(&self, match_id: Uuid, fact: MatchFact) -> Result<(), CoreWriteError>;
    async fn delete_fact(&self, match_id: Uuid, fact_id: Uuid) -> Result<(), CoreWriteError>;
}

#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait TeamWriteRepository: Send + Sync + std::fmt::Debug {
    // read（削除の参照整合判定の材料）
    async fn count_matches_referencing_team(&self, team_id: Uuid) -> Result<u32, CoreWriteError>;
    async fn count_facts_referencing_player(&self, player_id: Uuid) -> Result<u32, CoreWriteError>;
    // write（素朴 CRUD。delete_team は所属選手の cascade 削除を 1 save で含む）
    async fn save_team(&self, team: Team) -> Result<(), CoreWriteError>;
    async fn delete_team(&self, team_id: Uuid) -> Result<(), CoreWriteError>;
    async fn save_player(&self, player: Player) -> Result<(), CoreWriteError>;
    async fn delete_player(&self, player_id: Uuid) -> Result<(), CoreWriteError>;
}

#[uniffi::export]
pub async fn record_append_fact(
    repo: Arc<dyn MatchWriteRepository>,
    match_id: Uuid,
    fact: MatchFact,
) -> Result<(), CoreWriteError>;   // load → validate_append → 合格時のみ repo.append_fact
```

**import の atomic commit 用に第 3 の trait を追加（2026-07-22 追記 — handball-project#83）**: 上記 2 本（`MatchWriteRepository` / `TeamWriteRepository`）は 1 操作 = 1 `context.save()` の素朴 CRUD で、逐次・非 atomic（決定 7）。import の commit を atomic 化する（決定 7 の 2026-07-22 追記）ため、**全書き込みを 1 トランザクションで束ねる粗い入口**を 1 本足す。素朴 CRUD の 2 本はそのまま温存し、import 経路だけがこの入口を使う。

```rust
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait ImportWriteRepository: Send + Sync + std::fmt::Debug {
    /// batch を 1 `context.save()` で保存する（全成功 or 1 件も保存しない）。
    /// 検証はコアが呼ぶ前に済ませる — この実装は検証なしの素朴バッチ。
    async fn commit_import(&self, batch: ImportWriteBatch) -> Result<(), CoreWriteError>;
}

/// 保存順序込みでコアが組み立てる書き込み集合。facts は `sort_by_persistence_order` 済み。
pub struct ImportWriteBatch {
    pub teams: Vec<Team>,      // 新規のみ（既存再利用は含めない）
    pub players: Vec<Player>,  // 新規のみ
    pub r#match: Match,
    pub facts: Vec<MatchFact>,
}
```

設計不変条件との整合（コールバック注入は崩さない）:

- **コールバック注入を維持**: Swift が実装する foreign trait を Rust が `Arc<dyn>` で受ける形は不変。発火の判断・順序・検証は Rust のまま、Swift は 1 context への insert + save だけを担う
- **stateless 境界（決定 6）**: batch を値で 1 回渡すだけで、トランザクションオブジェクトを FFI 越しに持ち回らない。「`begin_tx` → `save(tx)` → `commit_tx`」型は FFI を跨ぐ long-lived object になり決定 6 に反するため採らない（Considered options 参照）
- **DB 非所有（設計不変条件 1）**: `context` / `save()` は Swift のみ。トランザクション境界は DB ハンドルを握る Swift 側にしか張れず、この分担が atomic の前提
- **粗い境界（設計不変条件 4）**: N 回の append より粗い 1 バッチはむしろ理想形。V2 は単一 `ModelContainer` なので team / player / match / fact を 1 context に混ぜて save できる

これに伴い `commit_sample_match_import` は「計画（純粋）→ `validate_fact_log` で 1 回検証 → `ImportWriteRepository::commit_import(batch)` を 1 回」へ変わる（現行の `save_team` / `save_player` / `save_match` ループ + facts 逐次 `record_append_fact` を置換）。

### 2. 移管範囲 — 全書き込み経路をコア経由に（grill 確定 2026-07-19 で拡大）

当初案は「判断の厚い 3 経路のみ移管、薄い転送は残置」だったが、grill で**「書き込みは全部コア経由」の一貫性を優先**する判断に拡大した。薄い経路の移管自体は Swift を減らさないが、(1) 書き込みの目録がコア 1 箇所に揃い、(2) 可視性遮断（決定 3）を全書き込みに張れて「コアを通らない書き込み経路」が型システム上消滅し、(3) Android が全書き込みで同じ入口・同じ構造化エラーを得る。

| 経路 | 移管 | 内容 |
|---|---|---|
| fact 3 経路（append / update / delete） | **第 1 段** | validate → save 調停の所在を Swift 実装層からコアへ。`SwiftDataMatchRepository` は検証なしの素朴 CRUD（foreign trait 実装）になる |
| タイマーモードの phase 自動補完（`ensureTimerPhasesCovering`） | **第 2 段** | D-snap index・被覆差分・連鎖 append はドメイン規則を含む発火 orchestration の典型。「記録操作 in → 補完 append 込みの発火」をコア入口に |
| タイマー→動画移行の commit（`MigrateToVideoStore.commit`） | **第 3 段** | anchor 変換（SegmentResolver — 既に Rust）と保存順序設計（config 先行 save → fact 逐次 update）をコアへ |
| match ヘッダ保存 / team・player CRUD | **第 4 段** | コア入口（passthrough + 削除の使用中判定）へ。削除の参照整合判定は**コアが持つ**（下記）。呼び出し元（store / `EditMatchViewV2` / importer / migrator）は呼び先をコア入口へ機械的に置換（フォーム判断はシェルのまま） |
| サンプル試合 import の merge 調停 + commit | **第 5 段**（2026-07-20 追加） | 起草時は「シェル残置」と判断したが撤回した（下記）。merge 候補算出を純粋関数、import commit を write orchestration としてコアへ |
| 観測（`observeMatches` / `observeTeams`） | 残置 | 書き込みではない。AsyncThrowingStream は境界に乗せず、SwiftData の通知機構と密結合のまま |

**import 経路の merge 調停もコアへ（2026-07-20 追記 — handball-project#67）**: 起草時（および ADR 0004 決定 2）は「repository / DB に触る `MatchImporterV2` / `MatchMergerV2` の調停はシェルの責務のまま」としていたが、棚卸しの結果この判断を撤回する。`MatchMergerV2`（約 230 行）は背番号 + 正規化名の exact / partial 照合・候補ソート・default decisions・名前カノニカライズという**プラットフォーム非依存の純粋ドメインロジック**で、DB には一切触らない（呼び出し側が snapshot を渡す形）。`MatchImporterV2` も decode 本体はコア呼び出し済みで、残っていたのは team / player の ID 割当と保存順序 — 第 1〜4 段で移した発火 orchestration と同型のものだった。Android / Web で確実に再実装が必要になるため、Swift オラクルが生きているうちに移す（#56 / 第 1〜4 段と同じ論法）。

境界の形は既存 2 層をそのまま踏襲する:

- **計画層** `sample_import`（純粋関数・feature 非依存）— `find_team_options` / `default_decisions` / `normalize_name` / `import_commit_plan`（DTO + decisions + 事前生成 ID in → 保存すべき entity と発火順 out）
- **発火層** `ffi_write::commit_sample_match_import` — 計画 → 新規 Team save → 新規 Player save → Match save → facts を逐次 `record_append_fact`（検証つき）

従属して次を固定する:

- **ID 供給は `Vec<Uuid>` 方式**（決定 4 の同型。新規 fact を組むわけではなく `recorded_at` は DTO 由来のため `NewFactStamp` は不要）。必要数は `sample_import_required_id_count` がコア側で数え、不足は `InsufficientNewIds` で拒否する
- **`TeamOption.id`（SwiftUI `Identifiable` 用の `UUID()`）はコアに持たせない** — コアは UUID を生成しない（設計不変条件 2）。Swift シムが `existing?.id` ベースの計算プロパティで再提供する
- **`ExistingSnapshot` は `playersByTeamID` マップではなく平坦な `players: Vec<Player>`** に畳む。`Player.team_id` が所属の一次情報であり、呼び出し側が team ごとに `loadPlayers(teamId:)` した結果を詰めていたので両表現は等価
- **表示名の解決（existing なら DB の現名称、createNew なら DTO 名）はシェル残置**。検証入力ではない read（`load_team`）を repository 契約へ足さないため（決定 1 の「read は最小セット」）。コアが返すのは ID と件数集計（`ImportCommitOutcome`）まで
- decode 失敗（未知の teamKey / playerKey 等）は `CoreWriteError::ImportDecodeFailed` で拒否する（移植元 `ImportError.conversionFailed` 相当）。計画が失敗した時点で 1 件も発火しない

**削除の使用中判定はコアへ（grill 確定 2026-07-19）**: `deleteTeam` / `deletePlayer` の参照整合（`teamInUse` / `playerInUse`）は、trait の参照カウント read（`count_matches_referencing_team` / `count_facts_referencing_player`）をコアが読み、使用中なら発火せず構造化エラーで拒否する形に移す。従属して次を固定する:

- **Swift 実装側のチェックは削除**（コアと二重にしない）。「最下層の防衛線」の役割は、可視性遮断（決定 3）による「全書き込みが必ずコアを通る」に置き換わる
- **cascade（チーム削除時の所属選手削除）は trait `delete_team` 実装内に残す** — 判断ではなくストレージ操作のセマンティクスであり、1 `context.save()` の原子性を保つ
- チェックと削除が 2 FFI 呼び出しに分かれるため理論上の時間窓は広がるが、現行も context 間の直列化保証はない（fetchCount → save の同一 context 逐次実行のみ）。**保証クラスは best-effort のまま変わらない**ことを明記して受け入れる

### 3. 強制力 — 生 repository を core 境界の内側に隠す（ADR 0001 却下理由 1 への回答）

「全書き込みが検証を通る」をシェル規約ではなく **Swift の可視性で強制**する: 書き込みメソッドを `MatchRepository` / `TeamRepository` プロトコル（RecorderApplication 公開面）から外し、素朴 CRUD の foreign trait 準拠実装は composition root（`RecorderV2Services`）だけが知る。store / view / importer はコア入口（またはそれを包む thin service）しか呼べなくなる。repository 内包方式（現行）は「実装を差し替えれば検証が消える」のに対し、この形は**コアを通らない書き込み経路が型システム上存在しない**。移管の段に合わせて段階的に外す（第 1 段で fact 書き込み、第 4 段で残り全部）。この遮断が、削除の使用中チェックを Swift 実装から外す（決定 2）ことの前提になる「最下層の防衛線」の置き換えでもある。

### 4. ID・時刻の供給契約 — 設計不変条件 2（決定性）は維持

コアは引き続き `now()` / UUID 生成を持たない。発火経路でコアが新規 fact を組む場合（phase 自動補完）は、**必要数関数 + スタンプバッチ**方式とする（grill 確定 2026-07-19。sample_dto の `required_id_count` 前例と同型 — 消費順・必要数の知識をシェルへ漏らさない）:

- シェルは `(id, recorded_at)` のペア（`NewFactStamp` record）を生成して渡す。現行の「fact ごとに fresh な `UUID()` / `Date()`」をペア単位で保つ（時刻の同値化はしない — パリティ維持）
- **必要数はコアが数える**: 記録入口と同じ読み取り（repo 経由）で「この記録に必要な補完 fact 数」を返す関数を公開し、シェルはその数だけスタンプを生成して記録入口を呼ぶ
- 数え時と発火時の間に fact 列が変わり不足した場合は、発火せず**構造化エラー**（`InsufficientNewIds { required, provided }`）で拒否（安全網。シェルは再試行できる）
- foreign trait での ID 供給 callback は採らない（決定性と再現テスト可能性を守る）

### 5. エラー体系 — `CoreWriteError`（ADR 0002 整合）

```rust
#[derive(uniffi::Error)]
pub enum CoreWriteError {
    ValidationFailed { issues: Vec<DomainValidationIssue> },  // 発火せず拒否
    TeamInUse { match_count: u32 },                            // 参照整合: 使用中チーム削除の拒否
    PlayerInUse { fact_count: u32 },                           // 参照整合: 使用中選手削除の拒否
    Repository { message: String },                            // シェル実装の失敗（診断文字列のみ）
    InsufficientNewIds { required: u32, provided: u32 },       // ID 供給契約違反
}
```

- ユーザー向け文言は持たない（文言はシェル所有 — ADR 0002）。`ValidationFailed` は既存の `DomainValidationMessage`（シム）でそのまま表示化できる
- `From<uniffi::UnexpectedUniFFICallbackError>` を実装し、Swift 実装の panic 相当を `Repository` へ畳む（panic=abort 構成でのクラッシュ防止 — スパイク欄参照）
- Swift 側 adapter は SwiftData の throw を `CoreWriteError.Repository(message:)` に写像する

### 6. 並行性・再入・キャンセル（ADR 0001 却下理由 3 への個別回答）

- **隔離**: foreign trait は `Send + Sync` 必須。既存 SwiftData 実装は「操作ごと fresh `ModelContext` + `OSAllocatedUnfairLock`」で `Sendable` 準拠済み — そのまま満たす
- **寿命**: `Arc<dyn>` はシェル参照が尽きれば解放（uniffi 管理）。コア側に repository を**保持する object を作らない**（毎呼び出しで受け渡す stateless 形を保つ）ことで循環参照を構造的に排除
- **再入**: repository 実装からコアの write 入口を呼び戻さない（規約 + レビュー観点。実装は素朴 CRUD なので自然に満たされる）
- **キャンセル**: uniffi 非対応。write 入口は短時間・非中断で設計し、キャンセルに依存しない（現行の save 群も同様）

### 7. 挙動パリティ — 保存の単位・順序・観測は「改善しない」

- 1 操作 1 `context.save()`、連鎖（補完 append・migrate commit）は逐次・非 atomic、途中失敗は再実行復旧前提 — 現行挙動を移管後も維持する（トランザクション化は将来の別判断）
- fact CRUD が `observeMatches` を re-emit しない現挙動も維持（`MatchListStore.reloadItems` 補完の構図を変えない）

**import 連鎖だけ atomic へ（2026-07-22 追記 — handball-project#83）**: 上記のうち **サンプル試合 import の commit（第 5 段 / `ffi_write::commit_sample_match_import`）だけ**を「逐次・非 atomic」から「1 トランザクション（成功なら全保存 / 失敗なら 1 件も保存しない）」へ改める。適用範囲と理由:

- **対象は開発者専用経路に限る**: import to DB の唯一の導線は HandballRecorder の `DevDataViewV2`（アプリエントリで丸ごと `#if DEBUG`。App Store リリースビルドには存在しない）。エンドユーザーは踏まないため「ユーザー向けの失敗表示・復旧導線」は不要で、論点は純粋に「開発者が再 import で復旧するか / そもそも中途半端を残さないか」に絞れる
- **失敗時に「facts 0 件の試合行 + 孤児チーム」が残る問題**（#72 で実測、#83）を構造的に消す。現状の「途中失敗は再実行復旧前提」は孤児の後始末を開発者の手作業にし、再 import は重複 Match も生む（`doubleImportCreatesDuplicateMatch` で固定）
- **import はバッチだから自然に atomic 化できる**: record / phase 補完 / migrate と違い、import は commit 時点で全 fact を保有し、かつ `import_commit_plan` → `sort_by_persistence_order` で永続化順に整列済み。全 entity/fact を 1 `context.save()` に束ねられる。副産物として N-1 回の冗長な whole-log 検証と save が消える
- **record / phase 補完 / migrate commit の逐次・非 atomic は不変**（対話的・逐次で、全書き込みを事前に持たないため）。トランザクション化は import に限る

**検証の意味論差（プレフィックス → 最終形）を許容する**: 逐次 append は「各プレフィックスが `validate_fact_log` を通る」ことを要求する（だから `sort_by_persistence_order` が要る）。バッチ検証は「最終形の log が 1 回通る」だけを見るため厳密には**わずかに緩い**。import は**新規 Match への投入で fact log が空スタート**のため「保存瞬間の DB 真実 = バッチそのもの」となり、決定 1「検証入力は保存瞬間の DB 真実」は成立する。整列は引き続きコアが行う（読み出し規約 `SwiftDataMatchRepository.factRecordOrder` と一致させるため）。

### 8. wasm / CLI / Kotlin への影響

- foreign trait・async 入口・`async-trait` 依存はすべて feature `uniffi` 配下 — wasm / CLI ビルドは不変
- 計画層は純粋関数としてどのターゲットからも使える
- Kotlin（#59）は同じ trait が `interface` として生成され、Android は repository 実装（Room 等）を書くだけで発火 orchestration を共有する — 本 ADR の主目的の 1 つ

## 実装順序と完了条件

同一 feature ブランチ内でコミット順を分離する（ADR 0004 と同じ規律）:

1. **機構整備**: `MatchWriteRepository` foreign trait + `CoreWriteError` + fact 3 経路の write 入口（Rust 単体テストは fake repo 実装で計画層・拒否経路を固定）。ios_poc smoke に **async 往復のランタイム検証**（Swift 実装を Rust が await して結果が返る・エラーが写る）を追加 → **完了（2026-07-19）**: 計画層 `write`（roster 構築の 0 件 skip / 重複先勝ちルールをコアへ移管）+ 発火層 `ffi_write`。fake repo テストで合格時のみ発火・違反不発火・repository 失敗伝播を固定し、ios_poc で発火・`ValidationFailed` 写像・未知エラーの `Repository` 畳み込みをランタイム確認
2. **アプリ第 1 段（fact 3 経路）**: `SwiftDataMatchRepository` から validation を剥がし素朴 CRUD 化 → store をコア入口呼び出しへ → 可視性遮断（決定 3）→ アプリテスト green → **完了（2026-07-19）**: `MatchFactWriter`（コア入口を旧 repository と同形で包む単一入口）を新設し、store / importer / migrator / view の呼び先を置換。`MatchRepository` protocol から fact 3 メソッドを削除（遮断）。`CoreWriteError` は `DomainValidationFailure` 準拠でエラー表示経路を不変に維持。実装追記: uniffi 0.32 の async foreign trait 機構（`uniffiTraitInterfaceCallAsync*` が `Task {}` へ非 Sendable closure を渡す）が Swift 6 strict concurrency に通らないため、生成コードを含む `HandballToolkit` ターゲットのみ言語モード v5 でコンパイルする（uniffi 側の対応で外す）
3. **アプリ第 2 段（phase 自動補完）**: `ensureTimerPhasesCovering` をコアの記録入口へ移管（ID 事前生成契約）。該当 Swift テストを Rust テスト + 境界テストへ移植 → green → **完了（2026-07-19）**: 計画層 `phase_completion_plan` / `phase_completion_fact`（D-snap 判定・連鎖補完・明示 PhaseStart の素通しを純粋関数化）+ 入口 `count_phase_completion_facts` / `record_fact_with_phase_completion`（`NewFactStamp` バッチを消費順に使用、不足は `InsufficientNewIds`）。store の `ensureTimerPhasesCovering` と 5 箇所の guard 呼び出しを削除し、`appendAndReproject` のコア入口 1 呼び出しに集約（補完の要否はコアが判断 — 動画 / PhaseStart は素通し）。挙動は Rust 計画層・入口テストへ移植し、既存 store テスト群は境界回帰としてコア経由で green
4. **アプリ第 3 段（migrate commit）**: `MigrateToVideoStore.commit` の順序 orchestration を移管 → green → **完了（2026-07-19）**: 計画層 `video_migration_plan`（control の both 化・play の videoClock 変換・control → play の発火順を純粋関数化）+ 入口 `commit_video_migration`（config 先行 save → 逐次 validate → update）。シェル入力は `VideoSyncInput`（fact_id + video 区間）のみ — draft の matchClock は facts の read-only ミラーなのでコアが DB 真実から導く。実装追記: 計画不成立（sync 欠落・videoClock 導出不能）用に `CoreWriteError::MigrationPlanInfeasible { message }` を決定 5 の enum へ追加（wizard の事前 validation が通っていれば到達しない安全網）。store の `buildUpdatedFacts` / `updateControlFact` / `MigrationCommitError` を削除
5. **アプリ第 4 段（entity CRUD + 完全遮断）**: match ヘッダ / team / player の CRUD をコア入口へ、削除の使用中判定をコアへ移管（Swift 実装のチェックは削除）。importer / migrator / view の呼び先置換 → 可視性遮断を全書き込みに拡張 → green → **完了（2026-07-19）**: `TeamWriteRepository` foreign trait + `record_save_match` / `record_delete_match`（passthrough）・`record_save_team` / `record_delete_team`・`record_save_player` / `record_delete_player`（削除は参照カウント read → 判定 → 発火）。シェルの write 面は `MatchWriter`（旧 MatchFactWriter を改名 — fact に加え match ヘッダ save / delete / migration commit を持つ）+ `TeamWriter` の 2 本に集約し、`MatchRepository` / `TeamRepository` protocol から全書き込みメソッドを削除（完全遮断 — 決定 3）。`SwiftDataTeamRepository` の使用中判定は削除し cascade のみ実装内に残置。view の catch は `CoreWriteError.TeamInUse / .PlayerInUse` へ置換、`RepositoryError` の validationFailed / teamInUse / playerInUse は廃止
6. **回帰検証**: (a) アプリ側既存テスト全 green（ユニット + パッケージ）、(b) UI テスト実機 green、(c) `PRE_RELEASE_SMOKE_TEST.md` 完走、(d) TestFlight 配布 + 数日 dogfood（#56 から引き継いだ dogfood を兼ねる — #61 完了条件）
7. **アプリ第 5 段（import の merge 調停 + commit）**: 決定 2 の追記（2026-07-20）に基づく後追い工程 — handball-project#67。計画層 `sample_import` + 入口 `commit_sample_match_import` → Swift の `MatchMergerV2` / `MatchImporterV2` を薄い梱包材へ縮小 → 該当 Swift テストを Rust テスト + 境界テストへ移植 → green → **完了（2026-07-20 — #67）**
8. **import commit の atomic 化（第 5 段の後続）**: 決定 7 / 決定 1 の 2026-07-22 追記に基づく — handball-project#83。`ImportWriteRepository` foreign trait + `ImportWriteBatch` を追加し、Swift 実装は 1 `ModelContext` に全 insert → `save()` 1 回。`commit_sample_match_import` を「計画 → `validate_fact_log` で 1 回検証 → `commit_import(batch)` を 1 回」へ置換（`save_team` / `save_player` / `save_match` ループ + facts 逐次 `record_append_fact` を撤去）。fake repo テストで「途中 throw なら 1 件も保存されない（atomic）／正常時は全保存」を固定し、既存の import 境界テストを green に保つ。**dev 専用経路（`DevDataViewV2` / `#if DEBUG`）に限定し、record / phase 補完 / migrate の逐次・非 atomic は変えない**

各段は独立して出荷可能な状態を保つ（途中の段で止めても境界は整合する）。

## Considered options

- **何もしない（ADR 0001 の再検討トリガー = Android の痛みを待つ）** → 却下。前提変化 (a)〜(c)（文脈欄）により、調停の二重実装は「Android で将来痛む」ではなく「今すでに Swift 実装層に埋まっている」問題になった。Swift オラクルが生きているうちに移す方が挙動固定の安全網を使える（#56 の実績と同じ論法）
- **write-plan のみ（計画をコアが返し、発火はシェル）** → 単独では却下、内部表現として採用（決定 1）。発火ループ・順序設計・エラー処理の所在が Swift に残り、Android で発火側の二重実装が発生する。#61 の目的（シェルを薄く・発火をコアへ）を満たさない
- **移管を判断の厚い 3 経路に絞る（entity CRUD・importer は残置 — 起草時の当初案）** → 却下（grill 確定 2026-07-19）。薄い転送の移管は Swift を減らさない（ADR 0001 却下理由 2 のとおり）が、書き込み目録の単一化・可視性遮断の全面化・Android との全書き込み共有を優先し、梱包材の対価を払って全経路をコア経由にする（決定 2）
- **削除の使用中チェックを Swift 実装に残す（整合性の最下層防衛を維持する案）** → 却下（grill 確定 2026-07-19）。判定の単一所在をコアに揃える。原子性は cascade を trait 実装内に残すことで維持し、防衛線の役割は可視性遮断（決定 3）が引き継ぐ
- **import の merge 調停をシェル残置にする（起草時 / ADR 0004 決定 2 の判断）** → 却下（2026-07-20 撤回 — 決定 2 追記）。「repository / DB に触る」という残置理由が棚卸しで成り立たないと判明した（`MatchMergerV2` は snapshot を受け取る純粋関数で DB に触らない）。Android で確実に再実装が必要になる純粋ドメインロジックであり、残置の対価は二重実装そのものだった
- **import も逐次・非 atomic のまま維持する（決定 7 起草時の方針）** → 却下（2026-07-22 — #83）。dev 専用経路とはいえ「facts 0 件の試合 + 孤児チーム」の後始末が開発者の手作業になり、再 import は重複 Match を生む。import はバッチで全 fact を事前保有するため atomic 化のコストが低い（粗い入口 1 本 + 1 context save）。record / phase 補完 / migrate（対話的・逐次）は対象外
- **import の atomic 化にトランザクション handle を FFI 越しに持ち回る（`begin_tx` / `commit_tx`）** → 却下（2026-07-22 — #83）。fine-grained な per-entity 呼び出しと atomic を両立できるが、トランザクショントークンが FFI を跨ぐ long-lived object になり決定 6（stateless 境界）に反する。async foreign trait の寿命・再入問題も最頻経路の外で新たに踏む
- **import の失敗時に補償ロールバック（発火済みを `delete_*` で巻き戻す）** → 却下（2026-07-22 — #83）。コールバック注入は保てるが、巻き戻し自体が失敗しうるため**真の atomic ではない**（best-effort）。バッチ 1 save の方が単純かつ確実
- **import commit の結果に表示名を含める（`load_team` を repository 契約へ追加する案）** → 却下。表示名は検証入力ではなくプレゼンテーションで、read 面を広げる対価に見合わない。コアは ID + 件数集計まで返し、名前解決はシェルの既存 read 面（`TeamRepository`）で行う
- **existing_facts をシェルが引数で渡す（読み取り注入なしの最シンプル境界）** → 却下（grill 確定 2026-07-19）。検証入力が「保存瞬間の DB 真実」から「store の読み込み済みコピー」に変わり、コピーが古い場合（Mac マルチウィンドウ等）に現行より検証が緩くなるセマンティクス変更を伴う。write 経路内の最小 read セット（決定 1）は採用し、それを超える汎用 read 面の注入（`load_players` 全般・観測など）は引き続き見送り
- **同期 callback（async をやめる）** → 却下。SwiftData 実装は async であり、同期ブリッジは executor 詰まり・deadlock リスクを最頻経路に持ち込む
- **コアが DB を所有（rusqlite 等でコア内永続化）** → 却下。SwiftData 資産（migration・CloudKit 展望・既存 store）を捨てることになり、「fact ログの永続化は各 OS ネイティブ」の分担を壊す

## 設計不変条件の改定

本 ADR の accept をもって、toolkit CLAUDE.md の設計不変条件 1 を次のとおり改定する:

> 1. **状態を所有しない stateless コア** — コアは DB ハンドル・保存実体・UI 状態を所有しない。判断・計画（何をどの順に保存すべきか）は「fact 列 in → 導出結果 out」の純粋関数として置く。ただし **永続化の発火 orchestration**（注入された repository を await する薄い export 関数）は feature `uniffi` 配下の境界層として持てる（ADR 0005）。repository を保持する long-lived object は作らない

条件 2（決定性）・3（構造化エラー）・4（粗い境界）は不変。条件 4 の「fact 列 in → projection out」に「操作 in → 発火 out」の write 入口が加わるが、細かい getter 応酬を避ける原則は同じ。

## Consequences

- 「保存してよいか + 何をどの順に保存するか」の所在がコア 1 箇所になる。`SwiftDataMatchRepository` / `SwiftDataTeamRepository` は検証・参照整合判定を持たない素朴 CRUD になり、validation 呼び出しの実装知識（domain 化・roster 構築込み）と使用中チェックが Swift から消える
- `RecordingScreenStore` から phase 補完 orchestration が、`MigrateToVideoStore` から保存順序設計が消える（Swift 削減の実体）
- 代償として callback 梱包材（adapter 準拠・エラー写像・ID 供給）が数十行新設される — ADR 0001 却下理由 2 の対価をここで払う。判断の厚い経路（fact 3 経路・phase 補完・migrate commit）では Swift の純減、薄い経路（entity CRUD）では書き込み目録の単一化・可視性遮断・Android 共有という一貫性の対価と割り切る（決定 2）
- fact 書き込みの全経路が FFI async を 1 往復する（現行 +1 ホップ）。書き込みは人操作起点の低頻度イベントで、2Hz ホットパス（ADR 0004 決定 5）とは別系統 — 性能懸念なし
- Android（#59）は repository 実装を書くだけで、発火判断・補完・順序設計を Rust から得る
- 本 ADR で ADR 0001 の「保存コールバック注入 却下」は正式に置き換えられる（write-plan 比較の宿題も決定 1 で決着）
- サンプル試合 import（第 5 段 / dev 専用の `DevDataViewV2`）は atomic になり、途中失敗で孤児レコード（facts 0 件の試合 + 孤児チーム）が残らない。`commit_sample_match_import` は N 回の逐次発火から `ImportWriteRepository::commit_import` の 1 バッチ呼び出しへ縮み、N-1 回の冗長検証・save も消える。record / phase 補完 / migrate commit の逐次・非 atomic は不変（決定 7 の 2026-07-22 追記）

## 参照

- [ADR 0001](0001-boundary-api.md) — 保存コールバック注入の却下（2026-07-12）と再検討トリガー・write-plan 比較の宿題。本 ADR が置き換える
- [ADR 0002](0002-error-model.md) — 構造化エラー・文言のシェル所有（`CoreWriteError` が従う）
- [ADR 0004](0004-ios-full-boundary.md) — FFI 本境界（本 ADR はその上に発火層を足す）
- handball-project#61 — 本 ADR の起点 Issue（棚卸し詳細はコメント）
- handball-project#59 — Kotlin バインディング（発火 orchestration の共有先）
- handball-project#67 — 第 5 段（import の merge 調停 + commit のコア移管。完了）
- handball-project#83 — import commit の atomic 化（決定 7 / 決定 1 の 2026-07-22 追記・第 8 段）
