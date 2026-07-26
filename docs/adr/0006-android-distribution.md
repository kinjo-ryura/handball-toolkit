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
- handball-project#106（本 ADR）/ #59（切り出し元）/ #133（サンプルシェル）/ #135（`.aar` publish）/ #134（OSS 公開の前提物）
