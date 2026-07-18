# iOS シェル向け FFI 本境界 — ドメイン全型の UniFFI 公開

## Status

accepted（2026-07-18 起草、同日 grill 済み。handball-project#56「HandballRecorder の Rust コア差し替え」）

## 文脈

#49 の UniFFI PoC は「SAMPLE_DTO_V2 JSON in → summary JSON out」の 2 関数境界で経路実証のみを行った。#56 の go 判断（2026-07-18、判断材料は Issue コメント）により iOS アプリ本体のコア差し替えを行う。これには ADR 0001 の型目録・関数目録を UniFFI 越しに公開する本境界が必要になる。

判断材料となったアプリ側依存の全数調査（handball-project#56 コメント参照）の要点:

- アプリ側依存は Tests 除き 71 ファイル。うち約 50 は機械的書き換え（enum パターン・init 置換は生成型でも同形）
- **ドメイン型への外部 extension は 0 件** — 計算プロパティ（`FactAnchor.matchClock` 等、呼び出し約 40 箇所）はすべてパッケージ内 extension 由来。生成型への Swift 側シム extension で呼び出し側を無改修化できる
- **Codable は外部から未使用**（永続化は列単位の手書き Mapper、サンプル入出力は独自 DTO 経由）
- **Sendable は repository の async 境界で実依存**、Identifiable は Player / Team の ForEach 5 箇所
- **2Hz ホットパス**: 動画モードで 500ms ごとに `LiveMatchProjection` 再構築 + SwiftUI body 内で fact 件数分の resolver 参照

### スパイク検証（2026-07-18、UniFFI 0.32）

境界設計の前提となる生成品質を実験で確認した（検証コードは破棄済み。再現は本 ADR の記述で足りる）:

| 検証項目 | 結果 |
|---|---|
| record（struct）の準拠 | `Equatable, Hashable` + `Sendable` が付く |
| enum（associated values）の準拠 | 同上 |
| object の生成 | `open class` + `@unchecked Sendable`。メソッド呼び出し可 |
| **record 内に object ハンドル** | 生成可（`TimelineProjection { resolvedFacts, resolver }` を 1 record で写せる）。ただしその record は Equatable を失う |
| `Uuid` の custom type 写像 | `uniffi::custom_type!(Uuid, String, …)` + uniffi.toml の `custom_types` 設定で **Swift 側は `public typealias Uuid = UUID`（Foundation）** になる |
| `SystemTime` | Swift `Date` に自動写像（uniffi 組み込み Timestamp） |
| record のフィールド | `public var`（in-place 変更可）+ ラベル付き memberwise init |
| デフォルト引数 | `#[uniffi(default = None)]` / `#[uniffi(default = [])]` が Swift の `note: String? = nil` 等に写る |

## 決定

### 1. 境界形状 — 全型 UniFFI 化（JSON 境界の置換）

ADR 0001 の型目録（ids / clock / configuration / entities / facts / projection 出力型 / validation）を UniFFI record / enum として公開し、関数目録（projection builders / validators）を `#[uniffi::export]` 関数として公開する。PoC の JSON 2 関数境界は役目を終えるため廃止する（ios_poc ハーネスは本境界の smoke テストに改修）。

serde 層を境界の外側に置く原則（ADR 0001）は変わらない — UniFFI のマーシャリング（RustBuffer）は serde ではなく、コアの型がそのまま契約になる。

### 2. sample_dto も境界に含める — アプリの DTO 変換層を双方向とも Rust へ置換（grill 確定）

サンプル入出力の変換係（アプリの `SampleMatchConverterV2` 304 行 / `MatchExporterV2` 273 行）は Swift に温存せず、**双方向とも Rust に一本化**する:

- **DTO → ドメイン**: コアの `sample_dto::convert`（P6 移植済み・パリティ検証済み）を FFI 公開し、Swift の Converter を削除
- **ドメイン → DTO（export）**: Rust に新規実装。パリティ対象外の新コードになるため、**Swift の `MatchExporterV2` をオラクルにした golden 比較**（同一試合を両実装で export して JSON 一致）と **round-trip テスト**（export → import でドメイン値復元）で担保してから Swift 側を削除する
- **ID 注入は事前生成 `Vec<Uuid>` 方式**: `convert` のクロージャ注入（ADR 0003 §2）は FFI では callback interface を要し、ADR 0001 が保存コールバック注入を却下したのと同じ問題系（隔離・再入・寿命）を持ち込む。シェルが必要数（DTO から数えられる）の UUID を事前生成して配列で渡す形にし、callback なし・stateless を維持する

なお repository / DB に触る `MatchImporterV2` / `MatchMergerV2` の調停ロジックはシェルの責務のまま（純変換部分だけが Rust へ移る）。

### 3. 型の写像方式 — コア crate に feature-gate した uniffi derive（grill 確定）

コア crate `handball-toolkit` に feature `uniffi`（default off）を追加し、公開型に `#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]` 等を付ける。ffi crate はコア型を再エクスポートして scaffolding を集約する（uniffi のマルチ crate 構成。Mozilla application-services の実運用形）。

- 型定義は 1 箇所のまま（ミラー型の二重更新なし）。wasm / CLI ビルドは feature off のため uniffi が依存グラフから存在ごと消え、依存最小の原則を壊さない
- ミラー型方式（ffi crate に写し + `From` 変換、約 30 型 × 600〜900 行）は**フォールバック**として保持する。cross-crate の metadata 集約が実装初期の smoke で通らない場合に切り替える（境界契約は同一なのでシム・アプリ側は影響なし。退避コストは cfg_attr 行の削除のみ）

**実装追記（2026-07-18 smoke で確定）**: uniffi は複数 crate（namespace）が同じ Swift `module_name` を共有すると生成ファイル（.swift / ヘッダ / modulemap）を上書き衝突させるため、「ffi crate に export 関数を置いて scaffolding を集約」は成立しなかった。実装では **export 関数もコア crate の feature-gated `ffi_api` モジュールに置き、namespace をコア 1 つに集約**する。ffi crate は staticlib 化 + bindgen CLI 同居のみの packaging 殻となる。境界契約・シム・アプリ側への影響はない（ミラー型への退避も不要だった）。

### 4. メソッド持ち型の扱い — 「データは record、繊細なロジックは object、自明なアクセサはシム」

| 分類 | 対象 | 写し方 |
|---|---|---|
| プレーンデータ | clock / configuration / entities / facts / projection 出力型の大半 | record / enum。フィールドをそのまま公開 |
| 自明な計算プロパティ | `FactAnchor` の 5 アクセサ、`MatchConfiguration` の 4 helper（`contentKind` は非公開のまま — ADR 0001）、`SummaryProjection` 系の導出値（`shotAttempts` / `scoringRate` 等）、`ScoreProgressionPoint.diff` | **Swift シムの extension で再実装**（数百行の純アクセサ） |
| 繊細なロジック | `SegmentResolver`（build + 参照系 8 メソッド） | **UniFFI object（Arc ハンドル）**。`build` は constructor、参照系はメソッド（`Option<&T>` 返しは所有値返しに変更）。`phases()` / `segments()` は Vec を返すメソッドとして公開 |
| ハンドル入り projection | `TimelineProjection { resolvedFacts, resolver }` | record（resolver フィールドは object ハンドル。スパイク検証済み） |

**シム再実装の許可基準（grill で研ぎ直し）** — 次の 3 条件をすべて満たすものに限る:

1. **入力は self のみ**（他の fact・他の segment・コレクション全体を参照しない）
2. **ループ・再帰・探索がない**（enum case の場合分けとフィールド取り出し・四則演算まで）
3. **ドメイン規則を含まない**（半開区間・優先順位・丸め方向・閾値のような「仕様として決めた」ものに触れない）

直感基準は「**型定義を見ればテストなしで正しさをレビューできるもの**」。この基準で `FactAnchor.matchClock`（self の case 取り出し）や `scoringRate`（自明な 0 除算ガード）は通り、`segment(forVideoElapsed:)`（探索 + 半開区間規則）は弾かれる。

**シムには最小限の Swift 単体テストを付ける**（削除される RecorderDomainTests から該当アクセサのテストを間引き流用。数十行想定）。写経ミス（case の取り違え等）は Rust 側 140 テストの守備範囲外のため。

### 5. ホットパス設計 — object ハンドル方式（grill 確定）

動画モードの 2Hz tick（`LiveMatchProjection.buildVideoMode`）と body 内の resolver 参照は、fact 列を再マーシャリングせず**ハンドル + スカラー引数**で渡す:

```rust
#[uniffi::export]
impl SegmentResolver {
    #[uniffi::constructor]
    pub fn build(facts: Vec<MatchFact>) -> Arc<Self>;
    pub fn resolve_match_clock(&self, video: VideoClock) -> Option<MatchClock>;
    // …参照系は self + スカラーのみ。FFI 越えは µs オーダー
}

#[uniffi::export]
pub fn build_live_match_video_mode(
    match_: Match,
    timeline: TimelineProjection,   // resolver はハンドルなので facts の再送なし
    current_video_clock: Option<VideoClock>,
) -> LiveMatchProjection;
```

object は**不変の導出スナップショットへのハンドル**であり、コアが状態を所有するのではない（設計不変条件 1「stateless 純粋関数コア」は維持。fact が変わったらシェルが作り直す）。tick 時 FFI ゼロの材料化テーブル方式は、性能問題が実測されたときの後付け最適化として温存する。

### 6. ID・時刻・コレクションの写像

| コア | FFI 上 | Swift 側 |
|---|---|---|
| `Uuid`（MatchId 等の type alias） | custom type（String bridge） | `UUID`（Foundation）。シムで `public typealias MatchID = UUID` 等を再提供し、呼び出し側の型注記を無改修化 |
| `DateTime<Utc>` | `SystemTime` に変換して公開 | `Date` |
| `BTreeSet<PlayerId>`（RosterSelection） | ソート済み `Vec<Uuid>` | シムで `Set<UUID>` を受け / 返す convenience init + computed property（既存 init シグネチャ維持） |
| `usize`（phase_index） | `u32` | シムで `Int` 変換 property |

### 7. validation / エラーの公開

- `DomainValidationIssue` を record として公開し、`validate_append` / `validate_update` / `validate_delete`（+ `RosterContext`）を関数公開する（ADR 0001 の関数目録どおり）
- **文言レイヤ（`DomainValidationMessage` + `userMessage`）は RecorderDomain（Swift）からシム層へ移設**する。`RecordingErrorPresenter` が同モジュールから参照するため移設先はシム package 内で必然。ADR 0002 の「文言はシェル所有」が iOS シェルで実体化する（lookup key `(scope, code)` の安定性は既存の wire format テストが担保）

### 8. Swift パッケージ構成 — モジュール名は `HandballToolkit` に統一（grill 確定）

- 生成 Swift + シム + XCFramework binary target を 1 つの SwiftPM package にまとめ、**Swift モジュール名は `HandballToolkit`** とする（PoC の uniffi.toml のまま）。**ツールキットとしての枠づけ（「HandballRecorder の内部コア」ではなく「ハンドボール試合データのツールキット」— research doc）を Swift 名前空間でも一貫させる**ことを、差し替え diff の最小化より優先する
- アプリ側は `import RecorderDomain` 71 行の置換（機械的 sed）+ `Packages/RecorderDomain` の削除と新 package への付け替え（各 Package.swift の依存宣言含む）。`MatchID` 等の typealias はシムが提供するため import 行以外の呼び出しコードは無改修
- **配布はソースのみコミット方式**（grill 確定）: 生成 Swift + シムは HandballRecorder リポにコミット、**XCFramework は gitignore してスクリプト生成**（toolkit の `build_xcframework.sh`）。バイナリを git 履歴に積もらせない。単独 clone では bootstrap（toolkit checkout + スクリプト 1 回）が必要になるため、手順を HandballRecorder の CLAUDE.md に明記する。リモート binary target 化・バージョニングは #56 の「SwiftPM 配布整備」工程で別途
- XCFramework に **macOS スライス（`aarch64-apple-darwin`）を追加**する（HandballRecorderMac も利用者のため）

### 9. Kotlin（#59）との関係

本 ADR の型公開・関数公開はそのまま Kotlin バインディング生成に流用できる（uniffi.toml に Kotlin 設定を足すだけ）。#59 で残るのは Kotlin 側シム（アクセサ再実装）と Android ターゲットのビルド整備のみ。

## 実装順序と回帰検証の完了条件（grill 確定）

同一 feature ブランチ内で**コミット順を分離**し、bisect 可能性を保つ:

1. **FFI 実装**: コア crate の feature `uniffi` + derive 付与 → ffi crate で export（**cross-crate metadata の smoke を最初に確認**。詰まったらミラー型へ切替）。sample_dto の export 方向を Rust に新規実装（オラクル比較 + round-trip）
2. **XCFramework 更新**: 3 スライス（ios / ios-sim / macos）+ サイズ最適化（LTO / strip / panic=abort、実測記録）
3. **Swift シム**: アクセサ extension + typealias + 文言レイヤ移設 + Identifiable 付与（Player / Team）+ シム最小テスト
4. **アプリ差し替え（コア）**: HandballRecorder の feature ブランチで `Packages/RecorderDomain` を置換 → テスト green
5. **アプリ差し替え（DTO 層）**: Converter / Exporter を Rust 呼び出しに置換 → テスト green
6. **回帰検証**: 完了条件は次の 4 項目 — (a) アプリ側既存テスト全 green（ユニット + パッケージ）、(b) UI テストを実機（Kim iPhone）で green、(c) `docs/PRE_RELEASE_SMOKE_TEST.md` 完走、(d) TestFlight 配布 + 数日の実機 dogfood で問題なし。**App Store 出荷は含めない**（観測 window 外で別途判断 — #56）

テスト資産: `RecorderDomainTests`（約 2,500 行）は Swift パッケージと共に**削除**する（守備範囲は Rust 側 140 テスト + wire format テストへ移管済み）。残すのはシムアクセサへの間引き流用分のみ。並走残置は二重コア保守の残存であり行わない。

## Considered options

- **JSON 境界の維持・拡張**（PoC の形のまま全 projection を JSON 化）→ 却下。型安全を失い、2Hz ホットパスで fact 列の全量シリアライズが毎 tick 発生する。ADR 0001 でコア API として却下した形を FFI でも採らない
- **DTO 変換層の Swift 温存**（機械的書き換えのみ、Rust への沈め込みは Android 実装時に再判断）→ 却下（grill 確定）。変換係は純粋ロジックで「コアへ沈めてシェルの二重実装を減らす」対象の典型であり、Swift オラクルが生きているうちに置換する方が golden 比較の安全網を使える。工程は同一フェーズ・コミット分離で回帰切り分けを担保
- **ミラー型 + From 変換（ffi crate 内で完結）** → フォールバックに格下げ。コアを uniffi 非依存に保てる利点はあるが、約 30 型の写しと変換 impl（600〜900 行）が新たな写経ミス面積になり、以後の型変更が恒久的に 3 箇所更新になる。feature-gate 方式なら single source of truth を保てる
- **コアに無条件で uniffi 依存** → 却下。feature-gate との差は cfg_attr を書く手間約 30 行のみで、wasm / CLI ビルドに無駄な依存が常駐するデメリットだけが残る
- **remote type 宣言（`#[uniffi::remote]`）** → 不採用。自 workspace の crate には feature-gate derive の方が直接的。third-party 型（Uuid / SystemTime）のみ custom type / 組み込みで写す
- **材料化テーブル方式（セグメント表を record で返し、参照系を Swift シムで再実装）** → 却下。tick 時の FFI はゼロになるが、半開区間 + degenerate 特例・running 優先（保存すべきセマンティクス 5 / 6）の再実装リスクを負う。object ハンドルの FFI 呼び出しコスト（µs オーダー × 2Hz）は許容範囲で、リスクに見合わない。性能問題の実測時に後付け最適化として再検討
- **Swift モジュール名 `RecorderDomain` 維持**（import 71 行の無改修・リバート容易性を優先する案）→ 却下（grill 確定）。得られるのは diff 最小化だが、ツールキットの枠づけと Swift 名前空間の不一致が恒久に残る。import 置換は機械的 sed で安く、枠づけの一貫性を採る
- **XCFramework をコミット / ビルドフェーズで都度生成** → 却下。前者はバイナリ十数 MB × 更新回数が git 履歴に不可逆に積もる。後者は毎ビルドに Rust toolchain を課し「iOS 開発体験で代金を払う」を最も濃く体現する

## Consequences

- コア crate に feature `uniffi`（default off）が入る。依存 crate 最小の原則は feature off のビルド（wasm / CLI）で維持
- PoC の JSON 2 関数（`toolkit_version` を除く）と ios_poc ハーネスは本境界へ移行・改修
- 差し替え完了時に Swift の `RecorderDomain` パッケージ実装・`RecorderDomainTests`・アプリの DTO 変換層（Converter / Exporter）は削除され、コアと変換係は Rust 単一になる（以後のドメイン変更は Rust が一次。Swift テストの逆移植運用は終了）
- Rust 側に export（ドメイン → DTO）方向が新設される（オラクル比較 + round-trip で担保）。以後 SAMPLE_DTO_V2 スキーマの変更は Rust 1 箇所の修正で全シェルに波及する
- `TimelineProjection` は Swift 側で Equatable を失う（object フィールドのため）。使用実態なし（全数調査で確認済み）
- HandballRecorder の単独 clone は bootstrap（toolkit checkout + XCFramework 生成スクリプト）が必要になる（CLAUDE.md に手順明記）
- 本境界は #59（Kotlin）の前提成果物になる

## 参照

- [ADR 0001](0001-boundary-api.md) — 型目録・関数目録・保存すべきセマンティクス（本 ADR はその公開手段を定める）
- [ADR 0002](0002-error-model.md) — エラー体系（文言のシェル所有が iOS で実体化）
- [ADR 0003](0003-parity-verification.md) — パリティ検証（export 新設のオラクル比較はこの手法の再適用）
- handball-project#56 — go 判断と依存全数調査（2026-07-18 コメント）
- handball-project#59 — Kotlin バインディング（本境界の流用先）
- `handball-project/docs/research/handballrecorder-rust-core.md` — 差し替え判断の背景
