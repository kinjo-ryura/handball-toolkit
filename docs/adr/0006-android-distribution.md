# Android 配布境界 — 実機 `.so` ビルド経路と Kotlin バインディングの確定範囲

## Status

accepted（2026-07-26 起草。handball-project#106「Android 実機ビルド（.so）と Kotlin/Android 境界の ADR 化」）

## 文脈

#59 で Kotlin バインディング生成とドメイン全型の Android コンパイル確認（staticlib = ELF AArch64、NDK 不要）まで済ませ、実機 `.so` 生成は「Android NDK 取得と SDK ライセンス同意という重い前払いが要り、確認以上の情報が今は得られない」として意図的に寝かせた（リポの「拡張判断のはしご — 今決めないこと」に沿う）。

2026-07-26、Android シェル実装（#133）の前提として着手トリガーが到来したため、実機 `.so` の生成経路を通し、その過程で決まる範囲を本 ADR に固定する。**本 ADR が扱うのはビルド・配布の経路**であり、境界の型・関数目録は ADR 0001、iOS 側の公開方式は ADR 0004 が持つ。

前提として、Android 実機は手元に無い。実行確認は Apple Silicon 上の arm64-v8a エミュレータで行う（実機と同 ABI）。

## 決定

### 1. NDK / SDK はこの repo の flake ではなくホスト環境が提供する

`flake.nix` に Android NDK / SDK を入れず、ホスト（dotfiles の nix-darwin）がグローバルに提供したものを「あれば使う」。

- 理由: Android 環境は他プロジェクトでも使う。SDK closure は実測 10.9 GiB（NDK 4.7G / system image 4.3G / emulator 1.1G）あり、リポごとに抱えるサイズではない
- 代償: この repo 単体では Android ビルドが再現しない。**Nix で宣言的**というリポの方針から一歩外れることを認識した上での判断。ホスト側に要求するインターフェースは `ANDROID_NDK_ROOT` 環境変数のみに絞り、依存を最小化する
- `ANDROID_NDK_ROOT` が無い環境では Android 関連の設定を一切行わない。**Android 以外のビルド（host / iOS / wasm）は影響を受けない**

### 2. クロスリンカは devShell の環境変数で渡す — `.cargo/config.toml` に直書きしない

`flake.nix` の `shellHook` が `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` と `CC_aarch64_linux_android` を export する。

- `.cargo/config.toml` の `linker` は**環境変数を展開しない**ため、直書きすると nix store の絶対パスが固定される。`nix flake update` のたびに壊れ、かつ `/nix/store/...` や `/Users/<name>/...` という**ユーザー固有パスが git 管理下に入る** — OSS 公開（#134）で持ち出せない
- 安定シンボリックリンク（`~/.local/share/android-ndk`）をホスト側に用意する案もあるが、それも利用者ごとのパスであることに変わりはない。環境変数経由なら repo 側に個人環境が漏れない
- **NDK clang を PATH に出さないこと**が要件。ホストリンクは Xcode CLT の `/usr/bin/cc` に任せる方針（`flake.nix` の `overrideAttrs` コメント / ADR 0004）なので、クロスリンカはフルパスで名指しする

リンカの API レベルは **24** を採る。Android シェルの minSdk 暫定値であり、確定は #133。NDK r29 は 21〜35 を提供しているため、上げ下げは `shellHook` の 1 箇所で済む。

### 3. `crate-type` に `cdylib` を追加（staticlib と共存）

```toml
crate-type = ["lib", "staticlib", "cdylib"]
```

UniFFI Kotlin バインディングは JNA が実行時に `dlopen` する形が標準配布形のため、Android には動的リンク（`.so`）で届ける。`crate-type` はターゲット別に切り替えられないので iOS / host ビルドでも `.dylib` が併せて生成されるが、リンク時間の微増のみで、XCFramework に入るのは staticlib のまま（配布物は不変）。

### 4. `panic = "abort"` を Android スライスでも維持する

ワークスペース `[profile.release]` の `panic = "abort"`（ADR 0004 実装順序 2）を Android でも共有し、**Android 専用プロファイルを作らない**。

`unwind` に戻すと uniffi の `catch_unwind` が復活し、コアの panic が Kotlin 例外になる。iOS では非 throws 関数がどのみち `fatalError` で落ちるため実質差が小さかったが、Android では開発体験・クラッシュ診断に差が出る。それでも abort を採る理由:

- 設計不変条件 3「エラーは構造化して返す」の下で、**panic は「起きてはいけないバグ」の領域**。validation は `DomainValidationIssue` を返す設計であり、panic を Kotlin 例外にすることは、本来通ってはいけない経路を握りつぶせる道を作る方向に働く
- iOS と同じプロファイルで配布物を作る方が、両シェルのバイナリ挙動を揃えられる
- サイズ実測（後述）で unwind は **+157 KiB (+12.4%)**。絶対値としては小さいが、これと引き換えに得られる診断性の価値は #133 まで未検証

**再検討トリガー**: #133 で Kotlin から実際に呼び、panic 時の診断（ネイティブクラッシュのみでスタックが辿れない等）が実際に苦しいと分かったとき。切替は下記 3 行の追加とビルド時の `--profile` 指定のみで、手戻りはほぼゼロ:

```toml
[profile.release-android]
inherits = "release"
panic = "unwind"
```

### 5. 対応 ABI は `arm64-v8a` 単独で開始する

- 開発機が Apple Silicon のため **AVD も arm64-v8a** になり、エミュレータと実機が同 ABI。1 スライスで両方カバーできる
- `x86_64` が要るのは Intel Mac か x86 CI で回す場合のみで、現時点でどちらの予定も無い。`armv7`（32bit）は新規開発でサポートする理由が無い
- 追加は `rust-toolchain.toml` の `targets` とリンカ設定への 1 行ずつで済む。**必要になってから足す**

### 6. `package_name` は既定（`uniffi.handball_toolkit`）のまま据え置く

Swift 側の `module_name = "HandballToolkit"`（ADR 0004 決定 8）に対応する Kotlin 側の決定だが、**本 ADR では確定させない**。

- Kotlin の package_name は publish 時の座標（groupId / artifactId）と揃えるのが自然で、**publish 先（Maven Central / GitHub Packages / リポ直置き）が #135 で未確定**。今決めると二度手間になる
- #133 のサンプルシェルは自前の import なので、既定のままでも進行を妨げない
- **確定トリガー**: #135（prebuilt `.aar` publish）で配布先を決めるとき

`uniffi.toml` の Kotlin custom_types（`Uuid` / 各 ID newtype → `java.util.UUID`、`CoreInt` → `Int`）は #59 で設定済みで、Swift 側と API 面が揃っている。本 ADR で変更しない。

### 7. ADR 0001「将来の境界拡張候補」の取り込みは行わない（#133 のトリガー待ち）

`missing_timer_phases(facts, config, seconds) -> Vec<PhaseStartPayload>`（現 Swift `RecordingScreenStore.ensureTimerPhasesCovering` 約 35 行）は、ADR 0001 が「Android シェル実装時に二重実装になる筆頭候補」と名指ししたものだが、**本 ADR の時点では取り込まない**。

理由は、Android シェルのコードがまだ 1 行も無く、二重実装の痛みが仮説でしかないため。ADR 0001 が「保存コールバック注入」「write-plan パターン」に置いた再検討トリガー（= Android シェルで実際に痛みを体感したとき）と同じ規律を適用する。境界への追加は純粋関数を 1 本足すだけで既存境界を壊さないため、#133 で書いてから判断しても手戻りは無い。

## 実装追記（2026-07-26 検証完了）

`cargo build --release -p handball-toolkit-ffi --target aarch64-linux-android` で `.so` 生成を確認:

- **`file`**: `ELF 64-bit LSB shared object, ARM aarch64, version 1 (SYSV), dynamically linked, not stripped`
- **実行時依存（`llvm-readelf -d`）**: `libdl.so` / `libc.so` **のみ**。`libc++_shared.so` に依存しないため、APK に STL を同梱する必要が無い（Rust コアの利点がそのまま出た形）
- **エクスポート**: UniFFI シンボル 214 個。namespace は `handball_toolkit` 1 つ（ADR 0004 決定 3 実装追記のとおりコア crate に集約されている）。`ffi_handball_toolkit_uniffi_contract_version` を含む
- **サイズ実測**（stripped / not-stripped）:

  | プロファイル | stripped | not-stripped |
  |---|---|---|
  | `panic = "abort"`（release） | 1,269,672 B (1.21 MiB) | 1,721,016 B |
  | `panic = "unwind"`（比較用） | 1,426,808 B (1.36 MiB) | 1,962,152 B |

  差分 **+157,136 B (+12.4%)** — 決定 4 の判断材料。比較用プロファイルは計測後に削除した

ホスト側の回帰（dotfiles 側で検証済み・#106 の要件）: `which cc` は `/usr/bin/cc`、`xcrun --show-sdk-path` は Xcode CLT の SDK、`$CC` / `$CXX` / `$SDKROOT` は空、NDK clang は PATH に出ていない。

エミュレータは dotfiles 側で `pixel-api36`（pixel_7 / API 36 / google_apis / arm64-v8a）の作成・起動・`adb devices` 到達を確認済み。`.so` を載せた実行確認は Gradle プロジェクトを要するため #133 の範囲。

## 実装追記（2026-07-28 — #133 のサンプルシェルで実証）

`examples/android`（Room + 3 trait の 15 メソッド + 最小 UI）をエミュレータ上で通し、保留していた 3 点と minSdk が決着した。サンプルの詳細は [`examples/android/README.md`](../../examples/android/README.md)。

**決定 4（`panic = "abort"` 維持）→ 維持で確定**。再検討トリガーは「Kotlin から呼んで panic 診断が実際に苦しいと分かったとき」だったが、実装を通して panic に到達しなかったため発火しない。`unwind` へ倒す積極的理由が現れなかったので、iOS と同一プロファイルのまま据え置く。あわせて、サンプルでは `.so` を strip しない（`packaging.jniLibs.keepDebugSymbols`）— abort 構成ではシンボルの有無がそのまま診断可否になるため。

**決定 6（`package_name` 既定据え置き）→ 変更なし**。サンプルは自前の import で足り、進行を妨げなかった。確定トリガーは引き続き #135。

**決定 7（`missing_timer_phases` の境界追加）→ 追加不要と判明**。ADR 0001 が「Android で二重実装になる筆頭候補」と名指ししたものだが、ADR 0005 実装順序 3 で phase 自動補完が `count_phase_completion_facts` / `record_fact_with_phase_completion` としてコアへ移っており、**Kotlin シェルは補完ロジックを 1 行も書かなかった**（サンプルの ① がその経路）。二重実装の痛みは発生しないため、この項目は解消として閉じる。

**minSdk は 24 で確定**（決定 2 の暫定値をそのまま採用）。ただし生成 Kotlin の API 面に `java.time.Instant`（`UtcDateTime`）が出るため、API 26 未満では **core library desugaring が必須**。NDK リンカの API レベル 24 と揃うことを優先し、desugaring を入れる側を採った。

新たに判明した制約と設計への影響:

- **エラー型のフィールド名に `message` を使えない**（コア側を修正済み）。uniffi の Kotlin backend は error 型を `sealed class … : kotlin.Exception()` として生成し `override val message` を必ず持たせるため、`message` という名前のフィールドは `Throwable.message` と衝突して**生成コードがコンパイルできない**。Swift は error を enum に落とすので露見せず、iOS だけでは気づけない。`CoreWriteError` の 3 variant と `SampleDtoError::InvalidJson` を `detail` へ改名し、`docs/ERROR_CODES.md` を追随させた（Swift シェル側の参照は 0 件 — ADR 0002 が「`message` をユーザーに見せない」と定めているため、シェルは case しか見ていなかった）
- **async foreign trait は代替形を検討するまでもなく通った**。Kotlin では `suspend fun` を持つ `interface` になり、Room の suspend DAO をそのまま実装にできる。coroutine scope はシェルが供給せず、生成コードが `GlobalScope.launch` で呼ぶ。Rust future の drop で Kotlin の `Job` が cancel される点は Swift 側（キャンセル非対応）と異なる。ただし生成コードが catch するのは `kotlin.Exception` までで `Error` は素通りするため、シェル側でも構造化エラーへ写像しておくのが安全
- **決定 5（2Hz ホットパス）の前提が Android では成り立たない**。ADR 0004 決定 5 は「object ハンドル + スカラー引数なら FFI 越えは µs オーダー」を根拠に per-call の参照系を許容したが、Android では JNA の `Structure` by-value マーシャリングが支配的で **1 呼び出しあたり 20〜50 µs**（fact 数にほぼ非依存）、fact 列を渡す呼び出しは **約 10 µs/fact** かかる。iOS の「描画のたびに fact 件数ぶん resolver を参照する」形は 305 件で 1 描画 約 11 ms になり成立しない。Android シェルは tick ごとの粗い呼び出し 1 本に畳むか、ADR 0004 が温存した**材料化テーブル方式**（`all_segments()` を 1 回引いて以後 Kotlin 側で解決）を採ること。実測値の表は README。**この実測が ADR 0004 の「性能問題が実測されたときに再検討」の発火にあたる**（エミュレータでの値であり、実機では改善する可能性がある）
- **Gradle は flake が提供する**（`pkgs.gradle`。closure 約 200 MB）。SDK / NDK をホストに委ねる決定 1 は変えないが、ビルドツールである gradle は wasm-bindgen-cli と同格として repo 側で宣言した。Gradle が読む `ANDROID_HOME` もホスト提供に加わる（決定 1 の「ホストへ要求するインターフェースは `ANDROID_NDK_ROOT` のみ」は `ANDROID_HOME` を含む形へ広がった）

## 実装追記（2026-08-01 — #135 で `.aar` 配布を確立）

`android/toolkit`（`com.android.library` モジュール）と `scripts/build_aar.sh` を追加し、prebuilt `.aar` を Maven Central へ配る経路を通した。保留していた決定 6 が確定し、利用者側の前提（Rust / Nix / NDK）が消えた。

**決定 6（`package_name`）→ `io.github.kinjoryura.handballtoolkit` で確定**。確定トリガー（「#135 で配布先を決めるとき」）が発火した。Maven 座標を `io.github.kinjo-ryura:handball-toolkit` と決めたので、Java パッケージ名に使えないハイフンを除去した形を採る。既定の `uniffi.handball_toolkit` は他の uniffi ライブラリと共有される名前空間であり、publish する成果物としては自分の座標下に置く（Swift の `module_name` を既定から変えた ADR 0004 決定 8 と同じ規律）。

**配布先は GitHub Release**（`.aar` を添付）。検討順は JitPack → Maven Central → GitHub Release。

- **JitPack を却下**: ビルダーが Linux なのに対し `flake.nix` は `system = "aarch64-darwin"` 固定（NDK clang のパスも `prebuilt/darwin-x86_64` 前提）。JitPack 上では `nix develop` が使えず、`rustup` + NDK を `jitpack.yml` で組む**第二のビルド経路**を抱えることになり、決定 1・2 と二重管理になる
- **Maven Central を見送り**: 利用側が `implementation("io.github.kinjo-ryura:handball-toolkit:0.1.0")` の一行で済む唯一の選択肢で、技術的には最良だった。実際に publish 設定（vanniktech プラグイン / POM / 署名 / sources・javadoc jar）まで組み、`publishToMavenLocal` で成果物が揃うことも確認した。しかし namespace 所有確認と GPG 署名が要り、**鍵の失効管理・パスフレーズ保管という継続的な負担**が発生する。外部シェル実装者がまだ 1 人も現れていない段階でこれを背負うのは投資として過大と判断し、撤回した
- **GitHub Release を採用**: `gh release upload` だけで配れ、鍵の管理はゼロ。**障壁除去の本体（Rust / Nix / NDK を不要にする）は同じく達成される** — 利用側に残る差は「`.aar` を `app/libs/` に置き、依存 2 行を自分で書く」だけ。**実際に使う人が現れたら Maven Central へ格上げする**（決定 5 の「必要になってから足す」と同じ規律。撤回した publish 設定は git 履歴に残してあり、復元は容易）

**サイズ実測**（決定 5 の arm64-v8a 単独構成）:

| 成果物 | サイズ | 備考 |
|---|---|---|
| `.aar` | 1,595,274 B (1.52 MiB) | 配布物本体 |
| ├ `classes.jar` | 1,088,644 B | 生成 Kotlin 665 クラス |
| ├ `jni/arm64-v8a/*.so` | 1,721,976 B | **strip しない**（決定 4 の診断性優先）。実装追記 2026-07-26 の not-stripped 実測 1,721,016 B と整合 |
| └ `proguard.txt` | 1,272 B | consumer ProGuard ルール（後述） |
| サンプル APK（debug） | 5,166,925 B | JNA の `libjnidispatch.so` 176,520 B を含む |

（撤回した Maven Central 経路で併産していた `sources.jar` 65,691 B / `javadoc.jar` 1,340,390 B は Central Portal の必須要件だったもので、Release 配布では作らない。格上げ時にまた要る）

新たに判明した制約:

- **consumer ProGuard ルールの同梱が要る**。UniFFI 生成コードは JNA の direct mapping（`Native.register`）で `.so` を解決し、JNA は実行時に reflection でクラス・フィールドを引く。消費側が R8 で minify するとこれらが削られ / 改名され、`UnsatisfiedLinkError` や `Structure` のフィールド不一致になる。サンプルは `isMinifyEnabled = false` なので**サンプルだけでは絶対に露見しない種類の壊れ方**であり、外部利用者の release ビルドで初めて出る。`consumer-rules.pro` を `.aar` に同梱し、利用者が何も書かなくても効くようにした
- **core library desugaring は消費側でも要る**。ライブラリ側での有効化は消費アプリへ伝播しない。生成 Kotlin の API 面に `java.time.Instant` が出る以上、minSdk < 26 の消費アプリは自分でも `isCoreLibraryDesugaringEnabled` と `coreLibraryDesugaring(...)` を書く必要がある（README に利用者向けの注意として明記）
- **`.aar` ファイル単体は依存情報を運ばない**。運ぶのは Maven の POM であり、Release に置いた `.aar` を `implementation(files(...))` で参照する形では POM が介在しない。そのため JNA と kotlinx-coroutines は**利用側が自分で宣言する必要がある**（README とサンプルの両方に明記した）。Maven Central へ格上げすればこの手間は消える — 格上げの利益はほぼこの一点に集約される
- **`scripts/build_android.sh` は役割を終えた**。サンプルが `.aar` を引く側に回り、「サンプルへ `.so` と生成 Kotlin を直接配置する」用途が無くなったため削除した。同じ生成は `build_aar.sh` がライブラリモジュール向けに行う

検証: サンプルアプリ（`examples/android`）を配布 `.aar` を `app/libs/` から参照する形へ切り替え、**`ANDROID_NDK_ROOT` 未設定・クロスリンカ未設定の状態でビルドが通ること**を確認した（外部利用者と同じ条件）。エミュレータ上で seed / ゴール記録 / サンプル試合の atomic import / 2Hz ホットパス実測まで実動作し、クラッシュ 0 件。Maven（`mavenLocal`）経由と `libs/` 経由で生成される APK は**バイト単位で同一**（5,166,925 B）だったため、配布方式の違いによる挙動差は無い。生成 Kotlin の import は `uniffi.handball_toolkit.*` から `io.github.kinjoryura.handballtoolkit.*` へ移っている。

## 実装追記（2026-08-08 — #136 でシムと文言を配布物に含めた）

`.aar` の中身を「生成 Kotlin + `.so`」から **「生成 Kotlin + 手書きシム + 文言リソース（en / ja）+ `.so`」** へ広げた。#135 で配布経路は通ったが、受け取った側が最初に書くコードが「`when` で埋めるアクセサ」と「39 ケース分のエラーメッセージ」だったため、そこを配布物側へ寄せた（#133 のサンプルシェルで実際に必要になったものの製品化 — リポの「拡張判断のはしご」に沿う）。

**Kotlin シムは ADR 0004 決定 4 の許可基準をそのまま適用**した。Swift シム 329 行に対応するのは 6 ファイルで、`src/main/kotlin/`（手書き）に置き、生成物の `src/generated/kotlin/` とはディレクトリで分ける。

移植しなかったものが 2 つある。いずれも Kotlin では言語機能が肩代わりする:

- **Identifiable 適合** — SwiftUI `ForEach` 専用の概念で、Compose は `key = { it.id }` のラムダを取る。`id` フィールドを持つ生成型はそのまま使える。ただし `TeamOption` だけは移植した（コアが UUID を生成しない以上、「新規作成」候補の識別子はシムが供給するしかない — ADR 0005 決定 2 追記）
- **CaseIterable 適合**（`PlayEventKind`）— Kotlin の enum は `entries` を標準で持つ

**API 名は 3 箇所だけ iOS シムと意図的に変えた**。Swift の enum case はメンバを持たないが Kotlin の sealed subclass は持つため、`FactAnchor.matchClock` / `videoClock`、`MatchConfiguration.phaseDurationSeconds` を基底型の拡張プロパティにすると `FactAnchor.Both` / `MatchConfiguration.Timer` の同名 non-null メンバと衝突し、**受け手の静的型で戻り値型（nullable か否か）が変わる**という静かな罠になる。null を返す側に `OrNull` を付けて避けた。iOS シムの命名は移植元 Swift API の呼び出し側を無改修に保つための制約だったが、Kotlin には合わせる先の既存 API が無いので、その制約は引き継がない。同じ理由で projection の導出値は `Int` へ narrow せず `Long` のまま返す。

**文言は「シェル所有」を崩さずに既定値だけを配る**。ADR 0002 決定 3 が文言を追い出した先は**コア（Rust）**であり、シム層に置くことは iOS でも既に決定済み（ADR 0004 決定 7 が `DomainValidationMessage` をシム package へ移設した）。Android では配布物がシム層を兼ねるため既定文言も `.aar` に入るが、**利用側アプリが同じ name の string を宣言すれば上書きされる**（Android のリソースマージはアプリが優先）ので、シェルの所有権は保たれる。写像を書き直さずに文言だけ差し替えられる形になった。

網羅性の担保は 2 段構え:

- **既定ロケールはコンパイラ**が見る。生成型は sealed なので、コアに case が増えると写像の `when` が非網羅になってビルドが落ちる（ADR 0002 決定 1 の「文言を書かない限りコンパイルが通らない」が Kotlin でも成立する）
- **追加ロケールと写経ミスはテスト**が見る。コンパイラは `values-ja` の足し忘れも「別 case の resource を指す取り違え」も検出できないため、`DomainValidationMessagesTest` がリソース XML を直接読んでロケール間の name 集合一致・接頭辞・全 case 分の存在・ケース数（3 / 2 / 22 / 12 / 7）を assert する

新たに判明した制約:

- **ライブラリのリソースは利用側アプリの名前空間へマージされる**ため、接頭辞なしの name は衝突事故になる。`resourcePrefix = "handball_toolkit_"` を宣言して lint に見張らせている
- **`CoreWriteError` は Kotlin 側で `CoreWriteException` に改名される**（uniffi の Kotlin backend が error 型を `sealed class … : kotlin.Exception()` にするため）。リソース名の scope は Rust 側の名前に合わせて `write` で統一した
- **シムのテストは JVM 単体テストで回る**（`.so` も端末も不要）。シムは生成 data class を組み替えるだけでネイティブに触らないため。25 テストが `gradle :toolkit:testDebugUnitTest` で通る

サイズへの影響は実測で `.aar` 1,595,274 B → **1,619,674 B**（+24,400 B、+1.5%）。内訳は `classes.jar` 1,088,644 → 1,107,990 B（+19,346 B）と `res/values*` の 27,067 B（非圧縮。zip 内では縮む）。サンプル APK（debug）は 5,201,946 B → **5,189,049 B** と逆にわずかに減った — シェル側の手書き文言（dex 内の文字列）が消え、共有のリソーステーブルへ移ったため。

**APK サイズを測るときは clean ビルドで測ること。** インクリメンタルな再パッケージでは entry が STORED のまま残ることがあり、同じ内容でも zip が 2.2 MB 膨らんだ状態が観測される（非圧縮合計は 10.5 MB で一致するのに zip が 5.2 MB と 7.4 MB に割れる）。#136 の計測中に踏んだ。

検証: `scripts/build_aar.sh` を通し、生成バインディングを作り直した状態でシムがコンパイルされること、`.aar` に `res/values/values.xml` と `res/values-ja/values-ja.xml`（string 93 件 = 46 ケース × 2 + 複数件の前置き 1）が入ることを確認した。

## 実装追記（2026-08-09 — #143 で Android 単体テストを CI に載せた）

#136 で入れた Kotlin シムと文言リソースの担保がローカル実行だけだったため、**JVM 単体テスト 25 件（`DomainValidationMessagesTest` 5 / `ShimAccessorsTest` 20）を CI の `check` ジョブに追加した**。`.aar` の組み立て（NDK 要）とサンプル APK のビルドは**載せない**。

**決定 1 は変更しない。** ランナー同梱の SDK を「ホスト環境が提供する」の一形態として使い、`flake.nix` には何も足していない。ランナーに SDK が無い場合のみ `android-actions/setup-android` を通す分岐を置いたが、実測では発火しなかった（後述）。

**NDK が要らないのは決定 3 の副産物**。`crate-type` に `cdylib` を足したため host ビルドでも `.dylib` が出る。uniffi の bindgen は library mode でライブラリ内のメタデータから生成するので、そこから Kotlin バインディングを作れる。**Android の `.so` から生成したものとバイト単位で完全一致する**ことを確認した（13,716 行 / 488,389 B）。生成 Kotlin はターゲット非依存であり、実機用 `.so` も端末も要らない — シムは生成 data class を組み替えるだけでネイティブに触らないため（実装追記 2026-08-08）。

したがって CI の追加ステップは 3 つで済む:

1. `cargo build --release -p handball-toolkit-ffi`（host。NDK 不要）
2. `uniffi-bindgen generate --library target/release/libhandball_toolkit_ffi.dylib --language kotlin`
3. `gradle -p android :toolkit:testDebugUnitTest`

**別ジョブにせず既存の `check` へ相乗りさせた**。nix のインストール（約 30 秒）と cargo キャッシュの復元を二重に払わずに済むため。

**ランナーの SDK 実測**（`macos-latest`）: `ANDROID_HOME=/Users/runner/Library/Android/sdk` が既に設定済みで、`platforms` に `android-36`（= `compileSdk`）、`build-tools` に `37.0.0`（= `buildToolsVersion`）がそのまま入っている。**追加インストールも AGP の自動ダウンロードも発生せず、コストは 0 秒**。NDK もランナーに同梱されているため、将来 `.aar` 組み立てを載せる判断になった場合も導入時間は要らない見込み。

**コスト実測**（いずれもキャッシュ温。既存ジョブへの上乗せ）:

| ステップ | キャッシュキー修正前 | 修正後 |
|---|---:|---:|
| host 向け cdylib をビルド | 45 s | 20 s |
| Kotlin バインディングを生成 | 76 s | 5 s |
| Kotlin シムの単体テスト | 14 s | 10 s |
| Gradle キャッシュ復元 | 5 s | 4 s |
| **上乗せ合計** | **2 m 20 s** | **39 s** |

CI 全体は **3 m 57 s** で、Android 追加前のベースライン（main 直近 5 回で 2 m 38 s 〜 4 m 07 s）の範囲に収まる。

**併せて cargo キャッシュキーを直した**。主キーが `Cargo.lock` のハッシュだけだと完全一致し、`actions/cache` が `Cache hit occurred on the primary key, not saving cache` で新しい成果物を保存しない。`target/` が最初の保存時点（debug のみ）で凍結され、release プロファイルと `--features bindgen` の成果物が毎回ゼロから作られていた。キーに `run_id` を足して毎回書き戻す形にしたところ、bindgen が 76 秒 → 5 秒になった。**#143 が持ち込んだ問題ではなく、元からあった CI の無駄**が Android ステップの追加で可視化された形。

**検出力の実証**: `values-ja/strings.xml` から string を 1 件削って回すと `DomainValidationMessagesTest > 既定ロケールと日本語ロケールで string の集合が一致する` が FAILED になる（`25 tests completed, 1 failed`）。守りたかった事故 — 既定ロケールの漏れはコンパイラが見るが**日本語側の漏れは実行時に黙って既定へ落ちる**（実装追記 2026-08-08）— が CI で赤くなることを確認した。

**`.aar` 組み立て（段階 b）とサンプル APK（段階 c）を載せない理由**:

- (b) は `build_aar.sh` が `ANDROID_NDK_ROOT` を要求し、決定 1 の射程をランナーへ広げる話になる。一方で (b) が追加で検出できるのは「クロスリンクとパッケージングの壊れ」だが、**それはリリース時に `build_aar.sh` を通す時点で必ず露見する**。CI で前倒しする価値は、リリース頻度が上がるまで小さい
- (c) はエミュレータ（system image 4.3 GB）の起動が要り、コストが段違いに大きい。サンプルは外部利用者向けの参照実装であり、壊れても配布物には影響しない

**再検討トリガー**: (b) を載せるのは、`.aar` のリリースが月次以上の頻度になったとき、または ABI を追加して（決定 5）クロスビルドの組み合わせが増えたとき。(c) は、サンプルが「動く参照実装」として外部から実際に参照され始めたとき。

## Considered options

- **`flake.nix` に NDK / SDK を入れて repo 完結にする** → 却下（決定 1）。closure 10.9 GiB を repo ごとに抱えることになり、他プロジェクトでも使う実態と合わない。再現性は「`ANDROID_NDK_ROOT` だけを要求する」形で最小限確保する
- **`.cargo/config.toml` に linker のフルパスを直書き** → 却下（決定 2）。ユーザー固有パスが git 管理下に入り OSS 公開で持ち出せない。cargo が config 値の環境変数展開に対応していれば第一候補だった
- **ホスト側に安定シンボリックリンクを張って直書き**（dotfiles 側が用意した `~/.local/share/android-ndk`）→ 却下（決定 2）。store パス固定は解消するが `/Users/<name>/` が残り、OSS 公開の問題は解決しない。リンク自体はホスト側の資産として有用なので残置
- **Android スライスだけ `panic = "unwind"`** → 見送り（決定 4）。トリガー付きで #133 へ送る
- **`x86_64` スライスも同時に作る** → 却下（決定 5）。Apple Silicon の AVD が arm64-v8a である以上、今は使う先が無い

## Consequences

- Rust コアを Android へ届ける経路が通り、#133（サンプルシェルで write 経路実証）の前提が満たされた
- この repo は Android ビルドについてホスト環境に依存する。`ANDROID_NDK_ROOT` が無い環境では Android ターゲットのみビルドできない（他ターゲットは無影響）。README に前提を記載する
- `panic` / `package_name` / `missing_timer_phases` の 3 点は #133 / #135 のトリガー付きで保留した。**#106 で「決めなかったこと」もここに記録されているため、後続で再発見する必要は無い**
- iOS / host ビルドで `.dylib` が余分に生成されるようになった（配布物は不変）

## 参照

- [ADR 0001](0001-boundary-api.md) — 境界 API 目録（型・関数の正典。「将来の境界拡張候補」= 決定 7 の対象）
- [ADR 0004](0004-ios-full-boundary.md) — iOS FFI 本境界（決定 9 が Kotlin との関係、実装順序 2 が release プロファイルの実測）
- `crates/handball-toolkit/uniffi.toml` — Kotlin バインディング設定（#59 で設定済み）
- handball-project#106（本 ADR）/ #59（切り出し元）/ #133（サンプルシェル）/ #135（`.aar` publish）/ #134（OSS 公開の前提物）/ #136（シムと文言の同梱）/ #143（単体テストの CI 化）
