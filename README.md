# handball-toolkit

ハンドボール試合データのツールキット。fact スキーマ + スコア / タイムライン導出（projections）+ validation を提供する Rust crate 群。

試合は **fact の追記専用ログ**として保存される — ゴールが決まった、phase が始まった、時計が止まった。それ以外のすべて（スコア、タイムライン、選手別スタッツ、UI が今どの操作を出してよいか）は、そのログから純粋関数で**導出**される。コアが所有するのはその導出と、「その fact を追記してよいか」を判定するルールだけで、それ以外は何も持たない。

単一のコアが iOS / Android / Web / CLI に供給される。[handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) が配信するスキーマの型付き実装であり、[HandballRecorder](https://github.com/kinjo-ryura/HandballRecorder) を動かしているコアでもある。

## 設計不変条件

この 4 つが crate のあらゆる API を規定している。シェル（このコアの上に載るアプリ）を書くなら、これが同意する契約になる。

**コアは状態を持たない。** DB ハンドルもセッションも UI 状態も所有しない。すべての入口は fact のスライスを受け取り、導出値を返す。永続化はプラットフォームの担当で、可搬なコアより遥かに上手くやる。

**コアは決定的。** `now()` を呼ばず、UUID も生成しない。時刻や ID が要る操作では、**呼び出し側が発行して渡す**。これがゴールデンテストの安定を担保し、wasm でも同じコードが動く理由になっている。

**エラーは構造化され、文章を持たない。** コアが返すのはコードとパラメータのみ。ユーザー向け文言をどの言語でも持たないので、多言語化はシェル側で閉じる。→ [docs/ERROR_CODES.md](docs/ERROR_CODES.md)

**境界は粗い粒度。** fact 列 in → projection out を同期 1 往復で行う。細かい getter の応酬をしないのは、呼び出しのたびに FFI / JNI / wasm の境界を越えるため。

## 使い方

```toml
[dependencies]
handball-toolkit = { git = "https://github.com/kinjo-ryura/handball-toolkit" }
```

### 書き込む前に検証する

validation は**リスト**を返す — 最初の 1 件で止まらず、見つかった問題をすべて報告する。非空なら書き込みを拒否する契約。

```rust
use handball_toolkit::validators;

let issues = validators::validate_fact_log(&facts, &match_);
if !issues.is_empty() {
    // 各 issue は (scope, code, params) の三つ組。文言は自前の表から引く
    // （コアは意図的に文言を持たない）。→ docs/ERROR_CODES.md
    return Err(issues);
}
```

より狭い検査には個別の入口がある: `validate_match` / `validate_configuration` / `validate_play_fact` / `validate_control_fact`、および write ガードの `validate_append` / `validate_update` / `validate_delete`。

### 表示するものを導出する

```rust
use handball_toolkit::projection::{SummaryProjection, TimelineProjection};

let timeline = TimelineProjection::build(&match_, &facts);
let summary = SummaryProjection::build_with_timeline(&match_, &timeline);
```

timeline を一度組んで渡し回すことで、segment の解決を二度やらずに済む。`ScoreProgressionProjection` も同じ形。`LiveMatchProjection` は「いま何ができるか」を答える記録中セッション向けの projection。

### 入力契約

**fact 列は永続化順（累積秒 → recordedAt → id）にソートしてから**渡すこと。未ソートのまま渡してもエラーにはならず、**黙って誤った結果を返す**。

```rust
use handball_toolkit::persistence_order;

persistence_order::sort_by_persistence_order(&mut facts);
```

### シェルが供給するもの

コアが決定的であることの裏返しとして、シェルが次の 4 つを所有する。

| シェルが供給する | 理由 |
|---|---|
| 時刻 | コアは時計を読まない |
| UUID | コアは生成しない。`required_*_id_count` に必要数を尋ね、その数だけ作って渡す |
| 永続化 | コアは書き込みを計画するだけで、実行はシェルの repository |
| ユーザーに見える文言すべて | コアが返すのはコードであって文章ではない |

## ターゲット

### Web (wasm)

配信 JSON からブラウザ内で projection を組み立てる。サーバーは要らない。

```bash
./scripts/build_wasm.sh   # → target/wasm/: .wasm + ES module の JS グルー + .d.ts
```

公開面は 3 関数だけで、「1 往復」の原則を保っている。

```js
import init, { requiredIdCount, buildMatchView } from './handball_toolkit_wasm.js';
await init();

const json = await (await fetch('.../v2/matches/foo.json')).text();
// コアは UUID を生成しないので、シェルが事前生成して渡す
const ids = Array.from({ length: requiredIdCount(json) }, () => crypto.randomUUID());
const view = JSON.parse(buildMatchView('foo', json, ids));
// view = { match, homeTeam, awayTeam, players, summary, timeline }
```

失敗は例外で返り、`message` に構造化エラーの JSON が載る。

`wasm-bindgen` crate と `wasm-bindgen-cli` は**バージョン完全一致**が必要なため、Cargo.toml 側を `=` でピン留めして flake が入れる版と揃えている。上げるときは両方同時に。

### iOS / macOS

```bash
./scripts/build_xcframework.sh   # → target/xcframework/: XCFramework + 生成 Swift API 層
./scripts/ios_poc/run.sh         # 本境界 smoke をシミュレータ内で実行
```

実機 / シミュレータ / macOS の 3 スライス構成。UniFFI の標準配布形に従い、「バイナリ + C モジュール」が XCFramework、生成された Swift API 層は利用側がソースとして一緒にコンパイルする 2 段構え。

### Android

**使う側に Rust / Nix / NDK は要らない。** [Releases](https://github.com/kinjo-ryura/handball-toolkit/releases) から prebuilt `.aar` をダウンロードし、アプリの `app/libs/` に置くだけで始められる。

```kotlin
dependencies {
    implementation(files("libs/handball-toolkit-0.1.0.aar"))

    // .aar ファイル単体は依存情報を運ばない（運ぶのは Maven の POM）ので、
    // この 2 つは利用側で宣言する。生成コードが Native.register で .so を dlopen する
    // のに JNA、suspend 関数に coroutines を使う。
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
}
```

`.aar` の中身は「生成 Kotlin + シム + 文言リソース + `arm64-v8a` の `.so` + consumer ProGuard ルール」。

##### シム — 生成型に付く自明なアクセサ

コアは「データ」だけを公開し、`when` 一発で書ける導出値は境界を越えさせない（FFI 呼び出し 1 回のコストに見合わないため — [ADR 0004](docs/adr/0004-ios-full-boundary.md) 決定 4）。その分の薄いアクセサは `.aar` が持っているので、利用側で書き直す必要はない:

```kotlin
val clock = anchor.matchClockOrNull          // when (anchor) { … } を書かずに済む
val rate = teamSummaryLine.scoringRate       // 試投 0 なら null
val source = configuration.videoSource       // Timer なら null
```

null を返しうるものに `OrNull` が付いているのは、`FactAnchor.Both` などの sealed subclass が同名の non-null メンバを持っており、同名にすると「受け手の静的型で戻り値型が変わる」ためである。

##### 文言リソース — validation / write エラーの既定文言

コアはユーザー向け文言を持たず、`(scope, code)` の構造化エラーだけを返す（[ADR 0002](docs/adr/0002-error-model.md) 決定 3）。**その写像の既定値を `.aar` が en / ja の 2 ロケール分持っている**ので、「最初に書くコードが 39 ケース分のエラーメッセージ」にはならない:

```kotlin
val message = issue.userMessage(context)     // DomainValidationIssue →
Text(message.title); Text(message.body)      //   title / body

try { … } catch (e: CoreWriteException) {    // ValidationFailed は
    show(e.userMessage(context))             //   issue 側の文言へ委譲する
}
```

**文言を差し替えたいときは、自分のアプリの `strings.xml` に同じ name を宣言するだけでよい**（Android のリソースマージはアプリ側が優先する）。写像を書き直す必要はない:

```xml
<!-- app/src/main/res/values/strings.xml -->
<string name="handball_toolkit_fact_negative_match_clock_title">時間が不正です</string>
```

name は `handball_toolkit_<scope>_<code>_title` / `_body`（`code` は snake_case）。コードの一覧は [`docs/ERROR_CODES.md`](docs/ERROR_CODES.md)。ロケールを足したい場合は `values-<locale>/strings.xml` に同じ name を並べる。

`CoreWriteException` の `detail` は**開発者向けの診断文字列であって UI に出さない**（ADR 0002 決定 5）。

利用側の前提は 1 つだけ。**minSdk が 26 未満なら core library desugaring を有効にすること** — 生成 Kotlin の API 面に `java.time.Instant`（`UtcDateTime`）が出るため:

```kotlin
android {
    compileOptions { isCoreLibraryDesugaringEnabled = true }
}
dependencies {
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
}
```

R8 の keep ルール（JNA と生成コードは reflection / direct mapping で引かれるため minify で壊れる）は `.aar` が consumer ProGuard ルールとして同梱しているので、利用側で書く必要はない。

シェルの書き方は [`examples/android/`](examples/android/) が参照実装になっている（Room による永続化 + 3 trait の実装 + 最小 UI）。**このサンプル自身が publish 済みの `.aar` を引く形**になっており、外部利用者とまったく同じ経路を通っている。

#### 同梱される OSS ライセンス一覧（利用側の表示義務）

`.aar` には、Rust コアがリンクする依存 OSS のライセンス一覧が入っている。

```
assets/handball_toolkit/third-party-licenses.json
```

**この `.aar` を組み込んだアプリを配る人が、エンドユーザーへの表示義務を負う。** `.aar` は Executable Form での配布にあたるため、受け取った時点で MIT / Unicode-3.0 の「著作権表示とライセンス本文を届ける」義務と、MPL-2.0 §3.2 の「ソース入手方法を知らせる」義務が利用側へ移る。同梱しているのは**その材料**であって、履行そのものではない。

- **届け方は問われない**（アプリ内画面・同梱テキスト・サポートサイトのいずれでもよい）。どのライセンスも媒体や UI を指定していない。ただしエンドユーザーが到達できる形にすること
- **一覧から項目を間引かない**。MIT が 19 件に分かれるのは crate ごとに著作権表示が違うためで、まとめると義務を満たさなくなる
- **`sourceUrl` を表示に含める**。MPL-2.0 §3.2 の告知はこれが担っている
- JSON の形（`schemaVersion` / `licenses[]` / `libraries[]` / `licenseIndexes`）は「ライセンス」節の「依存の OSS ライセンス表示」を参照

**この一覧は Rust コアの依存だけを含む。** JNA / kotlinx-coroutines / desugar_jdk_libs は Cargo.lock に現れないため入っていない。`.aar` は POM を運ばず利用側がこれらを自分で宣言する（上記のとおり）以上、**表示も利用側で別途用意する**こと。

#### 配布物をビルドする

```bash
./scripts/build_aar.sh   # → target/aar/handball-toolkit-<version>.aar
```

ABI は `arm64-v8a` 単独。生成 `.so` の実行時依存は `libdl.so` / `libc.so` のみで、`libc++_shared.so` の同梱は要らない。`.so` は **strip しない** — `panic = "abort"` 構成（[ADR 0006](docs/adr/0006-android-distribution.md) 決定 4）ではコアの panic がネイティブ abort になるため、シンボルの有無がそのまま診断可否になる。

**NDK / SDK はこのリポジトリの flake では提供しない**（他プロジェクトでも使うため、closure 約 11 GiB をリポジトリごとに抱えない判断 — ADR 0006 決定 1）。`.aar` をビルドするには、ホスト環境で Android NDK / SDK を用意し `ANDROID_NDK_ROOT` と `ANDROID_HOME` を設定する。設定されていれば devShell がクロスリンカを自動で構成する。未設定の場合に影響を受けるのは Android ターゲットのみで、Web / iOS / CLI のビルドは通常どおり動く。

#### バージョンの対応関係

配布物のバージョンは**コア crate の `version`（ワークスペース `Cargo.toml` の `[workspace.package]`）に従う**。

| | 値 |
|---|---|
| コア crate | `0.1.0` |
| `.aar` ファイル名 | `handball-toolkit-0.1.0.aar` |
| git タグ / Release | `v0.1.0` |

`build_aar.sh` はビルド前に `Cargo.toml` と `android/toolkit/build.gradle.kts` の値を照合し、不一致なら止める。上げるときは両方を同時に直して `v<version>` のタグを打つ。

#### リリース

GitHub Release に `.aar` を添付して配る。署名も外部アカウントも要らない。

```bash
./scripts/build_aar.sh                     # → target/aar/handball-toolkit-<version>.aar
gh release create v0.1.0 target/aar/handball-toolkit-0.1.0.aar \
  --title "v0.1.0" --notes "..."
```

**Maven Central（`implementation("io.github.kinjo-ryura:handball-toolkit:0.1.0")` の一行で済む形）は採らなかった** — namespace 所有確認と GPG 署名が要り、鍵の失効管理・パスフレーズ保管という継続的な負担が発生する。外部シェル実装者がまだ現れていない段階では、その負担に見合わないと判断した。障壁除去の本体（Rust / Nix / NDK を不要にする）は GitHub Release でも達成され、利用側に残る差は「`.aar` を `libs/` に置き、依存 2 行を書く」だけ。**実際に使う人が現れたら Maven Central へ格上げする**（判断の経緯は [ADR 0006](docs/adr/0006-android-distribution.md) 実装追記 2026-08-02）。

### CLI

配信されている試合 JSON を、コア自身の validators で検証する。

```bash
# v2 ルートを一括検証（index ↔ ファイル突合 + スコア / factCount / hasVideo / date の転記整合
#                    + index の date 降順 / slug 先頭日付 / factID 重複 / play・possession の anchor end）
cargo run -p handball-toolkit-cli -- validate ../handball-sample-matches/v2

# 単体ファイル。--json で機械可読出力
cargo run -p handball-toolkit-cli -- validate --json path/to/match.json
```

exit code は `0` = error なし（warning のみは 0）/ `1` = error あり / `2` = 使い方・パス誤り。severity は CLI 所有のレイヤ概念で、コアの構造化エラー（severity を持たず一律 blocking）とは別物。

この CLI は [handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) の CI（`.github/workflows/validate.yml`）から push / PR ごとに走り、配信 JSON の破損をそこで止める。CI は**この repo の main** を checkout してビルドするので、コア側で validators を強化すれば配信データの検証もそのまま追随する（逆に、既存の配信データが引っかかる規則を足すと向こうの CI が赤くなる）。手元実行は昇格前の事前確認や `--json` での調査に使う。

## 構成

```
crates/
  handball-toolkit/       コア（facts / clocks / configuration / entities / validators / projections）
                          feature `uniffi`（default off）で FFI 公開面が有効になる
  handball-toolkit-cli/   配信 JSON の検証 CLI
  handball-toolkit-ffi/   UniFFI パッケージング（staticlib 化 + バインディング生成）
  handball-toolkit-wasm/  wasm パッケージング（マーシャリングのみ・ロジックを持たない）
```

FFI と wasm の 2 crate はパッケージングに徹する。型と振る舞いはコアにあり、ラッパー側は独自のロジックを持たない。

## 開発

開発環境は Nix flake + direnv で宣言的に管理する（rustup は不使用）。ツールチェーンは [`rust-toolchain.toml`](./rust-toolchain.toml) で固定し、[rust-overlay](https://github.com/oxalica/rust-overlay) が提供する。

前提: Nix / direnv / Xcode Command Line Tools。リンクは Nix の clang ではなく CLT の `/usr/bin/cc` に意図的に任せている（iOS / XCFramework ビルドで `xcrun` 系と衝突させないため。詳細は `flake.nix` のコメント）。

```bash
direnv allow        # 初回のみ。以降はディレクトリに入ると自動で整う
cargo test          # 全テスト
cargo clippy
cargo fmt
```

direnv を使わない場合は `nix develop` で同じシェルに入れる。flake は `aarch64-darwin` に固定しているため、Nix 経路には現状 Apple Silicon が要る。CI も同じコマンドを macOS ランナーで回している。

## 正しさの担保

このコアは Swift 実装の移植であり、**移植元が仕様の正典**。正しさは 3 か所で固定している。

- **移植テスト** — 移植元のテストから 1:1 で写した 140 件
- **ゴールデンパリティ** — `handball-sample-matches` の実試合 JSON を両実装に通し、projection 出力が bit 単位で一致することを検証する。コーパス件数は `tests/golden_parity_tests.rs` の assert が正で、これが列挙漏れの検知も兼ねる
- **ワイヤ形式テスト** — シェルが文言表を引くキー `(scope, code)` を専用テストで固定する。改名は breaking change だから

`crates/handball-toolkit/tests/golden/inputs/` のフィクスチャは [handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) の配信データのコピーで、どのコミットから生成したかは `tests/golden/README.md` に記録がある。中身は試合の事実（スコア・時刻・イベント）で、テスト fixture としてのみ同梱している。

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/ERROR_CODES.md](docs/ERROR_CODES.md) | 全エラーコードとパラメータ・意味（**英語**。外部シェル実装者向けの参照表） |
| [docs/adr/](docs/adr/) | 設計判断: 境界 API / エラー体系 / パリティ検証 / iOS FFI 本境界 / write orchestration |
| [docs/PORTING.md](docs/PORTING.md) | Swift からの移植記録 |

ドキュメントとコードコメントは日本語で書く。例外は `docs/ERROR_CODES.md` のみで、これは外部実装者が文言表を書くための参照表なので英語に置いている。Issue / PR はどちらの言語でも歓迎。

## ステータス

移植は完走しパリティ検証済み。HandballRecorder は本番でこのコア上で動いている。Android は `.so` + Kotlin バインディングの生成と `examples/android/` の参照シェルまで通っており、wasm / CLI はビルド・テストが通っている。

crates.io へは未公開（当面は Git リポジトリを直接参照する）。

## ライセンス

MIT。[LICENSE](LICENSE) を参照。

### 依存の OSS ライセンス表示（配布物を作る人向け）

配布バイナリ（iOS の staticlib / Android の `.so`）には MIT / MPL-2.0 / Unicode-3.0 の OSS がリンクされる。**Executable Form で配る側にはライセンス本文と著作権表示を受領者へ届ける義務がある**（MPL-2.0 は加えて §3.2 の「ソース入手方法の告知」）。このリポはその一覧を [`THIRD_PARTY_LICENSES.json`](THIRD_PARTY_LICENSES.json) として持ち、**シェル側はこれを同梱して画面に出す**。

```bash
./scripts/generate_licenses.sh           # 生成して書き出す
./scripts/generate_licenses.sh --check   # 依存の現況と一致するか検査（CI が実行）
```

- 一覧は `cargo-about` が Cargo.lock から起こす。**手で書かない・手で直さない**。依存を足したら再生成してコミットする（忘れても CI の `--check` が落ちる）
- 許容ライセンスは [`about.toml`](about.toml) の `accepted`。ここに無いものが混ざると生成が失敗する。**落ちたときに安易に追記して通さない** — まずその依存を入れてよいかを判断する
- 生成物の形（`schemaVersion: 1`）:
  - `licenses[]` — ライセンス本文。同一本文は 1 件に集約する。MIT が 19 件に分かれるのは crate ごとに著作権表示が違うため
  - `libraries[]` — crate 一覧。`licenseIndexes` で `licenses[]` を参照する。1 crate が複数ライセンスに服することがある（`unicode-ident` は MIT と Unicode-3.0 の両方）
  - `sourceUrl` — crates.io の**当該バージョン**。MPL-2.0 §3.2 の告知をこれで満たす
  - `origin` — `"workspace"`（この repo の crate）か `"registry"`（外部）か。workspace メンバの判定は `cargo metadata` が行う
- **自前の workspace crate も一覧に残している**。外部利用者にとって `handball-toolkit` は third party であり、`.aar` を配る経路ではこちらの MIT 表示も必要になるため
- **`origin` は「自作かどうか」ではない**（handball-project#145）。誰から見て自作かは配布経路で変わる — この repo の作者にとってコアは自作だが、`.aar` を受け取った外部シェル実装者にとっては third party そのもの。同じ JSON が両方へ届く以上、**生成側は視点に依存しない事実だけを載せ、どう見せるかは各シェルが決める**。HandballRecorder は `origin == "workspace"` を「このアプリのコア」として独立セクションに出している
- **`origin` は `schemaVersion` を上げずに足した任意フィールド**。印を持たない版の一覧も読めなければならない（読み手は欠落を `"registry"` 相当として扱う）。**必須にするなら版を上げること**

iOS は `HandballRecorder` の `Packages/HandballToolkit/bootstrap.sh` がこの JSON をパッケージリソースへ取り込み、生成 Swift と同じく一致検証する。Android は `scripts/build_aar.sh` が `.aar` の `assets/handball_toolkit/third-party-licenses.json` へ同梱する（handball-project#142）。**`.aar` を受け取った側に何の義務が移るか**は「Android」節の「同梱される OSS ライセンス一覧」を参照。
