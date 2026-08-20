# Android サンプルシェル

handball-toolkit のコアを Android シェルから使う最小の参照実装
（[handball-project#133](https://github.com/kinjo-ryura/handball-project/issues/133)）。

**目的は「シェル契約が一目で分かること」**であって、プロダクション品質のアプリを示すことではない。
UI は plain View、エラー表示は素の文字列、依存は最小に絞ってある。iOS 側の参照実装
（HandballRecorder）は private かつ Swift なので、**公開できる参照実装はこれだけ**になる。

## シェルが実装するもの / コアが持つもの

境界の設計は [ADR 0005](../../docs/adr/0005-core-write-orchestration.md)（write orchestration）と
[ADR 0002](../../docs/adr/0002-error-model.md)（エラー体系）が正典。要約すると:

| | 持ち主 | このサンプルでの場所 |
|---|---|---|
| 「保存してよいか」の検証 | **コア** | — |
| 参照整合の判定（使用中チーム / 選手） | **コア** | — |
| 何をどの順に保存するかの計画 | **コア** | — |
| DB ハンドルとトランザクション境界 | シェル | `db/`, `RoomWriteRepositories.kt` |
| 検証入力の最小 read / 素朴 CRUD（15 メソッド） | シェル | `RoomWriteRepositories.kt` |
| ID / 時刻の発行 | シェル | `MainActivity.newStamp()`, `Seed.kt` |
| ユーザー向け文言 | シェル | `MainActivity.describe()` |

15 メソッドの内訳は `MatchWriteRepository` 8（read 3 + write 5）/ `TeamWriteRepository` 6（read 2 + write 4）/
`ImportWriteRepository` 1。**すべて `RoomWriteRepositories.kt` の 1 ファイルに置いてある** —
契約の全体を 1 画面で見せるため。

エラーコードの一覧と各 case の意味は [docs/ERROR_CODES.md](../../docs/ERROR_CODES.md)。

## ビルドと実行

このサンプルは **配布された `.aar` を `app/libs/` から参照する**（handball-project#135）。外部利用者と
まったく同じ経路を通すためで、`.so` や生成 Kotlin を自前で抱えることはしない。

前提:

- `nix develop`（または direnv）環境内にいること。Gradle は flake が提供する
- ホストが **`ANDROID_HOME`** を提供していること（SDK は repo の flake では持たない —
  [ADR 0006](../../docs/adr/0006-android-distribution.md) 決定 1）。**`ANDROID_NDK_ROOT` は要らない**
  — コアは `.aar` の中に .so として入っており、このサンプルは Rust をビルドしない
- arm64-v8a の AVD（ADR 0006 決定 5 のとおり ABI は arm64-v8a 単独）

まず `.aar` を `app/libs/` に置く。外部利用者と同じ経路を通すなら
[Releases](https://github.com/kinjo-ryura/handball-toolkit/releases) から落とす:

```sh
mkdir -p app/libs
gh release download v0.2.0 --pattern '*.aar' --dir app/libs
```

```sh
gradle :app:assembleDebug           # examples/android から
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.example.handballshell/.MainActivity
```

**コアを直したときは** `.aar` を作り直して差し替える（ここだけは NDK が要る）:

```sh
./scripts/build_aar.sh                                              # リポジトリルートから
cp target/aar/handball-toolkit-0.2.0.aar examples/android/app/libs/
```

`app/libs/` と `local.properties` はコミットしない。

### バージョンの対応関係

| | バージョン | 備考 |
|---|---|---|
| Gradle | 8.14.4 | flake の `pkgs.gradle` |
| AGP | 8.11.1 | Gradle 8.13+ を要求 |
| Kotlin | 2.1.21 | KSP と組で上げること |
| KSP | 2.1.21-2.0.1 | Kotlin と完全一致が必要 |
| Room | 2.7.2 | |
| handball-toolkit | 0.2.0 | `app/libs/handball-toolkit-0.2.0.aar`。コア crate の version に従う |
| JNA | 5.17.0（`@aar`） | 生成コードが `Native.register` で使う。**`.aar` は依存情報を運ばない**ので利用側で宣言する |
| compileSdk / targetSdk | 36 | `buildToolsVersion = "37.0.0"` を明示（nix の SDK には 1 つしか無い） |
| minSdk | **24** | 下記参照 |

## Android シェルを書く人向けの注意点

### 1. async foreign trait はそのまま通る。coroutine scope を渡す必要はない

`#[uniffi::export(with_foreign)]` + `async_trait` の foreign trait は、Kotlin 側では
**`suspend fun` を持つただの `interface`** になる。Room の suspend DAO をそのまま実装に使える。

呼び出しは uniffi の生成コードが `GlobalScope.launch` で行う（シェルが scope を供給する口は無い）。
副作用として:

- 実装は `Dispatchers.Default` 上で走る。メインスレッド DB アクセスにはならない
- Rust future が drop されると生成コードが Kotlin の `Job` を cancel する
  （Swift 側の「キャンセルは uniffi 非対応」とは事情が違う）
- 例外は `kotlin.Exception` までしか catch されない。**`Error` は素通りして
  GlobalScope の未捕捉例外になる**ので、実装側で握って構造化エラーへ写像しておくのが安全
  （`RoomWriteRepositories.kt` の `mapRepositoryFailure`）

### 2. エラー型のフィールドに `message` という名前を使わない

uniffi の Kotlin backend は error 型を `sealed class … : kotlin.Exception()` として生成し、
`override val message` を必ず持たせる。`message` という名前のフィールドがあると
`Throwable.message` と衝突して**生成コードがコンパイルできない**。

Swift は error を enum に落とすのでこの問題が起きず、iOS だけで開発していると気づけない。
コア側の診断文字列フィールドは `detail` に統一してある（#133 で改名）。

### 3. minSdk 24 では core library desugaring が要る

生成 Kotlin の API 面に `java.time.Instant` が出る（`UtcDateTime = java.time.Instant`）。
`java.time` は API 26 以降の標準ライブラリなので、minSdk 24 のままにするなら
`isCoreLibraryDesugaringEnabled = true` + `desugar_jdk_libs` が必要。

minSdk を 26 以上にするなら不要。このサンプルは ADR 0006 決定 2 が暫定値とした 24 を
そのまま確定値として採り、desugaring で対応している（NDK リンカの API レベルと一致させるため）。

### 4. 2Hz ホットパスは iOS と同じ設計では成立しない

ADR 0004 決定 5 は「object ハンドル + スカラー引数なら FFI 越えは µs オーダー」を前提に
per-call の参照系を許容している。Android では**桁が違う**。

実測（Pixel API 36 エミュレータ / arm64-v8a / **release ビルド** / 2000 回平均）:

| | facts 5 件 | facts 305 件 |
|---|---|---|
| `SegmentResolver.build` | 168 µs | 3,549 µs |
| `resolveMatchClock`（record 引数 + Option 戻り） | 49 µs/呼び出し | 36 µs/呼び出し |
| `phaseKind`（scalar 引数 + Option 戻り） | 26 µs/呼び出し | 22 µs/呼び出し |
| `allSegments`（材料化） | 56 µs | 29 µs |
| `buildSummary`（whole-log の粗い呼び出し） | 207 µs | 2,970 µs |

読み取れること:

- **per-call のコストは fact 数にほぼ依存せず 20〜50 µs で一定**。データ量ではなく
  1 回あたりの固定費（JNA の `Structure` by-value マーシャリング + RustBuffer の確保）が支配的。
  `resolveMatchClock` と `phaseKind` の差（約 15〜23 µs）が RustBuffer 1 往復ぶんの値段
- **fact 列を渡す呼び出しは約 10 µs/fact** でスケールする（`buildSummary` 207 µs → 2,970 µs、
  `SegmentResolver.build` も同様）
- したがって危険なのは iOS の「描画のたびに fact 件数ぶん resolver を参照する」形。
  305 件なら **1 描画あたり約 11 ms** になり、フレームを落とす

Android シェルでの指針:

- 描画パスの中で per-fact の resolver 呼び出しをしない
- tick ごとの粗い呼び出し 1 本（3 ms @305 件）に畳むか、`allSegments()` で表を 1 回引いて
  以後は Kotlin 側で解決する（ADR 0004 が「性能問題が実測されたときの後付け最適化」として
  温存した材料化テーブル方式。**その実測がこれ**）

計測時の注意:

- **debug ビルドで測らないこと**。debuggable なプロセスでは ART が `-Xcheck:jni` を有効化し、
  同じ計測が debug 126 µs / release 46 µs（`resolveMatchClock`）と 2.7 倍ずれる
- 端末側の CheckJNI（emulator の `userdebug` イメージは既定で有効）の影響は小さかった
  （46 µs → 43 µs）。支配的なのは JNA 側のマーシャリング
- **これはエミュレータでの値**。実機では改善する可能性がある（実機は未入手）

## サンプルの操作

起動すると 2 チーム × 3 選手と 1 試合（タイマーモード / 規定長 30 分）を seed する。
seed 自体もコア入口（`record_save_team` / `record_save_player` / `record_save_match`）経由。

| ボタン | 通る経路 |
|---|---|
| ① ゴールを記録 | `count_phase_completion_facts` → スタンプ生成 → `record_fact_with_phase_completion`。phase が無ければコアが自動補完する |
| ② シュート失敗を記録 | `record_append_fact` |
| ③ ポゼッション開始を記録 | `build_possession_fact` → `record_append_fact`。play / control のどちらでもない**第 3 の payload**（handball-project#154 / #184）で、teamId は必須・anchor は 1 本のみ |
| ④ 動画時刻の anchor で記録 | タイマーモードは matchClock のみ許可 → `ValidationFailed`（発火せず DB は不変） |
| ⑤ 使用中チームを削除 | `record_delete_team` → `TeamInUse`。判定はコア、シェルはカウントを返しただけ |
| ⑥ サンプル試合を import | `commit_sample_match_import` → `commit_import` を 1 トランザクションで |
| ⑦ 2Hz 相当パスを実測 | 上記の計測。結果は logcat にも出る（`adb logcat -s HandballShell`） |
