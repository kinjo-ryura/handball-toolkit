# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## リポジトリ概要

ハンドボール試合データのツールキット（Rust workspace）。[HandballRecorder](https://github.com/kinjo-ryura/HandballRecorder) のドメイン層 `RecorderDomain`（Swift・Foundation のみ依存の純粋計算 約 2,700 行）の移植であり、単一の共有コアを iOS / Android / Web (wasm) / CLI へ届けるための基盤。[handball-project](https://github.com/kinjo-ryura/handball-project) の submodule（`apps/handball-toolkit/`）として管理される。

- 経緯・設計判断の一次資料: [handball-project#49](https://github.com/kinjo-ryura/handball-project/issues/49) と `handball-project/docs/research/handballrecorder-rust-core.md`
- 設計の正典: `docs/adr/`（0001 境界 API / 0002 エラー体系 / 0003 パリティ検証 — accepted 2026-07-12。0004 iOS FFI 本境界 / 0005 write orchestration — accepted 2026-07-18。0006 Android 配布境界 — accepted 2026-07-26）。**各 ADR の「実装追記」が実装の現況を持つ**
- 境界のエラーコード一覧は [`docs/ERROR_CODES.md`](docs/ERROR_CODES.md)（外部シェル実装者向けの英語ドキュメント）。**エラー case を追加・改名したらこの表も更新する**（code は安定契約 — ADR 0002 決定 2）
- 移植の経緯・作業規律: [`docs/PORTING.md`](docs/PORTING.md)。**移植は完走済みで、同ファイルは完了記録**（現在地の管理台帳ではない）。進行中・未着手の作業は GitHub Issues が正
- ドキュメント・コードコメントは日本語で書く。**例外は [`docs/ERROR_CODES.md`](docs/ERROR_CODES.md) の 1 本のみ**（handball-project#134）— 外部シェル実装者が文言表を書くための参照表なので英語で保つ。README も含め他はすべて日本語（翻訳の二重管理を作らないため）

## 変更の出し方（main は保護されている）

**main へ直接 push できない。** ruleset [`protect-main`](https://github.com/kinjo-ryura/handball-toolkit/rules/19753789)（2026-07-26〜）が **直 push / force push / main の削除を禁止し、PR 必須 + CI の `check` ジョブ green 必須**にしている。bypass actor は無しなので**オーナーでも通らない**。docs 1 行の修正でもブランチを切る。

```bash
git switch -c docs/xxx                    # prefix は feat/ fix/ ci/ docs/ + 内容
# 変更して commit（メッセージ規約は親リポの global-commit skill）
git push -u origin HEAD
gh pr create --title "..." --body "..."   # 本文に関連 Issue（handball-project#NN）を書く
gh pr checks --watch                      # required = `check` ジョブのみ。macOS + Nix で 5〜7 分
gh pr merge --merge --delete-branch       # required approvals は 0 なので自分の PR を自分で merge できる
git switch main && git pull               # ローカル main を merge 後の状態へ追従させる
```

- **CI が落ちている PR は merge できない**。往復を減らすため push 前にローカルで `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` を通す
- **親リポの submodule pointer は merge 後の main を指す**（PR ブランチの commit を直接指さない）。push 順は **toolkit → 親リポ** — 逆にすると親リポがリモートに無い commit を指す
- **この保護は toolkit だけにある**: ruleset は public repo でないと無料プランで使えず、他の submodule（すべて private）は従来どおり main へ直 push する。「toolkit は PR 必須、他は直 push」の非対称を忘れないこと
- 導入の経緯と「force push がどうしても必要になったら ruleset を一時無効化する」という退避策は [`docs/PORTING.md`](docs/PORTING.md)「作業規律」

## 開発コマンド

開発環境は Nix flake + direnv で宣言的に管理する（rustup 不使用）。ツールチェーンは `rust-toolchain.toml` でバージョン固定し、rust-overlay が提供する。

```bash
direnv allow          # 初回のみ。以降はディレクトリに入ると自動で環境が整う
                      # direnv を使わない場合は nix develop

cargo test            # 全テスト
cargo test <部分一致>  # 単一テスト（テスト関数名の部分一致で絞り込み）
cargo clippy          # lint
cargo fmt             # フォーマット
```

- ツールチェーン更新は `rust-toolchain.toml` の `channel` を書き換えて `direnv reload`。wasm / iOS などのクロスターゲットも同ファイルの `targets` に足す
- **flake.nix に Nix の clang / apple-sdk を入れないこと**: リンクは意図的に Xcode CLT の `/usr/bin/cc` に任せている（将来の iOS ターゲット / UniFFI → XCFramework ビルドで xcrun 系と衝突させないため）。rust-overlay の propagation を空にしている `overrideAttrs` を外さない（詳細は flake.nix のコメント）

## アーキテクチャ

Cargo workspace。4 crate 構成:

- `crates/handball-toolkit/` — コア crate（facts / clocks / configuration / entities / validators / projections）。feature `uniffi`（default off）でドメイン全型の UniFFI derive と FFI 関数公開（`ffi_api` / `ffi_support`）が有効になる（ADR 0004。feature off の wasm / CLI ビルドでは uniffi が依存グラフごと消える）
- `crates/handball-toolkit-cli/` — sample-matches 配信 JSON（SAMPLE_DTO_V2）の検証 CLI（handball-project#58）。コアの validators を呼ぶだけの薄いシェルで、コアには手を入れない。使い方は README「検証 CLI」
- `crates/handball-toolkit-ffi/` — FFI パッケージング crate。staticlib 化（XCFramework の中身）と uniffi-bindgen CLI（feature `bindgen`）のみを担い、型・関数の公開面はコア crate の namespace に集約する（ADR 0004 決定 3 実装追記）
- `crates/handball-toolkit-wasm/` — wasm パッケージング crate（handball-project#57）。JS 向けの粗粒度エントリ（`toolkitVersion` / `requiredIdCount` / `buildMatchView`）とマーシャリングだけを担い、コアには触れない。**ID 生成はシェル（JS の `crypto.randomUUID()`）が行う** — コアは UUID を生成しない（設計不変条件 2）ので、この crate も乱数を引かず `getrandom` の wasm バックエンド設定が不要

Kotlin バインディングは専用の workspace member を持たない。生成設定は `crates/handball-toolkit/uniffi.toml` の `[bindings.kotlin]`（Uuid / 各 ID newtype / CoreInt の custom_types 込み）にあり、`.so` と Kotlin バインディングは `crates/handball-toolkit-ffi/` から `scripts/build_aar.sh` が生成し、`android/toolkit/` の Gradle モジュールが `.aar` に束ねる（handball-project#59 / #106 / #133 / #135 いずれも完了。配布境界は ADR 0006）。iOS シェル向けのドメイン全型 UniFFI 公開（本境界）は ADR 0004 で確定・実装済み。

### iOS 向け XCFramework（UniFFI）

```bash
./scripts/build_xcframework.sh   # target/xcframework/ に HandballToolkit.xcframework + 生成 Swift API 層
./scripts/ios_poc/run.sh         # 本境界 smoke をビルドして iOS シミュレータ内で実行
```

### Web 向け wasm（wasm-bindgen）

```bash
./scripts/build_wasm.sh   # target/wasm/ に .wasm + ES module の JS グルー + .d.ts
```

`wasm-bindgen` crate と `wasm-bindgen-cli`（flake が nixpkgs から入れる）は**バージョン完全一致**が必要。Cargo.toml 側は `=` でピン留めしてあるので、nixpkgs が上がったら両方を同時に合わせる（不一致は生成時の schema version mismatch で落ちる）。

XCFramework は ios / ios-sim / macos の 3 スライス構成（HandballRecorderMac も同じ枠組み）。サイズ最適化はワークスペース Cargo.toml の `[profile.release]`（LTO / codegen-units=1 / panic=abort。実測と代償は ADR 0004 実装追記）。生成 Swift（`HandballToolkit.swift`）は XCFramework に入らない。「バイナリ + C モジュール」が XCFramework、Swift API 層はソースとして利用側が一緒にコンパイルする 2 段構え（UniFFI の標準配布形）。

### Android 向け `.aar`（UniFFI + JNA）

```bash
./scripts/build_aar.sh   # → target/aar/handball-toolkit-<version>.aar
```

配布物は **GitHub Release に添付する prebuilt `.aar`**（handball-project#135）。中身は「生成 Kotlin + 手書きシム + 文言リソース（en / ja）+ `arm64-v8a` の `.so` + consumer ProGuard ルール」で、**利用者は Rust / Nix / NDK を一切要しない**。Gradle モジュールは `android/toolkit/`、リリース手順は README「リリース」。

**シムと文言リソース**（`android/toolkit/src/main/kotlin/` と `src/main/res/values*/`。handball-project#136）は手書きで、生成物の `src/generated/kotlin/` とはディレクトリで分かれている（後者は `build_aar.sh` が毎回消して作り直す）。シムの許可基準は ADR 0004 決定 4、文言の位置づけは ADR 0006 実装追記 2026-08-08。

**Maven Central は採らなかった** — namespace 所有確認と GPG 署名（鍵の失効管理・パスフレーズ保管という継続的負担）が要り、外部シェル実装者がまだ現れていない段階では見合わないため。障壁除去の本体は Release 配布でも達成される。**使う人が現れたら格上げする**（ADR 0006 実装追記 2026-08-02）。

**FFI 公開面か `.aar` 同梱物を変えたらリリースを積む**（handball-project#190）。対象は ① `ffi_api` / `ffi_support` の関数・型の増減・改名・挙動変更 ② `.aar` 同梱物（生成 Kotlin / シム / 文言リソース / ライセンス JSON / `.so`）の変化。どちらも Release から `.aar` を落とした利用者に見える差なので、main に入れたまま放置すると **README の案内と実配布物が黙って食い違う**（v0.1.0 は 35 コミット分ずれ、Kotlin シム・文言・ライセンス JSON・possession fact のどれも配布物に入っていなかった）。**該当する PR を merge したら、続けて次の版を切ること** — 手順は README「リリース」。**CHANGELOG ファイルは置かない。変更履歴は Release notes が正**（commit / PR が `handball-project#NN` を参照しており、写すと二重管理になるため）。

NDK / SDK は**この repo の flake ではなくホスト環境**が提供する（ADR 0006 決定 1）。`ANDROID_NDK_ROOT` があれば devShell の shellHook がクロスリンカを `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` に export する。未設定の環境では Android ターゲットだけがビルドできず、host / iOS / wasm は影響を受けない。

- ABI は `arm64-v8a` 単独（開発機が Apple Silicon で AVD も同 ABI。ADR 0006 決定 5）
- 生成 `.so` の実行時依存は `libdl.so` / `libc.so` のみ（`libc++_shared.so` の同梱は不要）。`.so` は strip しない（診断性優先 — ADR 0006 決定 4）
- 生成 Kotlin の package は `io.github.kinjoryura.handballtoolkit`（Maven 座標に対応。ADR 0006 決定 6 が #135 で確定）
- **NDK clang を PATH に出さないこと**: ホストリンクを Xcode CLT の `/usr/bin/cc` に任せる方針は Android でも変えない。クロスリンカはフルパスで名指しする
- **エラー型のフィールドに `message` という名前を使わないこと**: Kotlin backend は error 型を `sealed class … : kotlin.Exception()` として生成するため `Throwable.message` と衝突し、生成コードがコンパイルできない（Swift では露見しない。診断文字列は `detail` に統一 — ADR 0006 実装追記）
- **consumer ProGuard ルールを消さないこと**（`android/toolkit/consumer-rules.pro`）: JNA は reflection で引くため、消費側が R8 で minify すると壊れる。サンプルは `isMinifyEnabled = false` なので**サンプルでは絶対に露見しない**
- **`.aar` ファイル単体は依存情報を運ばない**: 運ぶのは Maven の POM で、Release 配布（`implementation(files(...))`）では POM が介在しない。JNA と kotlinx-coroutines は**利用側が自分で宣言する必要がある** — README とサンプルの両方に明記してあるので、依存を増減したら 3 箇所（`android/toolkit/build.gradle.kts` / README / `examples/android/app/build.gradle.kts`）を揃えること
- **validation / write の case を増やしたら文言を 2 ロケール分足すこと**（`src/main/res/values/` と `values-ja/`）。既定ロケールの漏れは写像の `when` が非網羅になってコンパイルが落ちるが、**`values-ja` の漏れはコンパイラに見えない**（実行時に既定ロケールへ黙って落ちる）。`gradle -p android :toolkit:testDebugUnitTest` の `DomainValidationMessagesTest` が検出する（handball-project#143 で CI にも載せたので、叩き忘れても push すれば CI が止める）
- **リソース名には `handball_toolkit_` 接頭辞を付けること**（`resourcePrefix` が lint で見張る）: ライブラリのリソースは利用側アプリの名前空間へマージされるため、接頭辞なしは衝突事故になる
- **シムに探索やドメイン規則を書かないこと**: 許可されるのは「self のみ / ループ・再帰・探索なし / ドメイン規則を含まない」の 3 条件を満たすものだけ（ADR 0004 決定 4）。半開区間・優先順位・丸め・閾値に触れる計算はコアに置く

### Android サンプルシェル（`examples/android/`）

Room + 3 trait の 15 メソッド + 最小 UI の参照実装（handball-project#133）。**公開できるシェル実装はこれだけ**（iOS 側は private かつ Swift）。ビルド手順・Android 固有の落とし穴（async foreign trait の取り回し / minSdk と desugaring / 2Hz ホットパスの実測値）は [`examples/android/README.md`](examples/android/README.md)。

**このサンプルは配布された `.aar` を `app/libs/` から参照する**（#135）。外部利用者と同じ経路を通すためで、NDK 無しでビルドできる。コアを直したら `./scripts/build_aar.sh` の出力を `examples/android/app/libs/` へコピーする。Gradle は flake が提供、SDK はホスト（`ANDROID_HOME`）。

### 設計不変条件（コアに入れてよいもの / いけないもの）

1. **状態を所有しない stateless コア** — コアは DB ハンドル・保存実体・UI 状態を所有しない。判断・計画（何をどの順に保存すべきか）は「fact 列 in → 導出結果 out」の純粋関数として置く。ただし**永続化の発火 orchestration**（注入された repository を await する薄い export 関数）は feature `uniffi` 配下の境界層として持てる（ADR 0005）。repository を保持する long-lived object は作らない
2. **決定性** — `now()` / UUID 生成をコアに置かない。timestamp / ID はシェルが発行して fact に載せて渡す（ゴールデンテストの安定と wasm 対応のため）
3. **エラーは構造化** — エラーコード + パラメータのみを返す。日本語等のユーザー向け文言をコアに焼き込まない（文言は各シェルが持つ）。移植元の `DomainValidationMessage` をそのまま写さないこと — ここは意図的な再設計ポイント
4. **境界は粗い粒度** — FFI / JNI / wasm 越えを前提に、細かい getter の応酬ではなく「fact 列 in → projection out」の同期バッチ形状を保つ

### 移植のオラクル（Swift 実装）とパリティ検証

**オラクルは凍結済み。「Swift が真実の仕様」は移植面にのみ適用される** — この 2 点を取り違えると、Rust 独自に進化した挙動を「オラクルと不一致だから」と誤って巻き戻す。

- **オラクルの現在地**: RecorderDomain は HandballRecorder main から削除済み（`8aeffb8`「アプリのコアを HandballToolkit へ差し替え」2026-07-18）。sibling submodule `../HandballRecorder/` を見ても無い。到達手段は tag `oracle-dump-final` からの取り出しのみ:

  ```bash
  # ../HandballRecorder/ で。main を汚さずに読むため worktree を使う
  git worktree add /tmp/oracle oracle-dump-final
  ls /tmp/oracle/Packages/RecorderDomain/Sources/RecorderDomain/   # Clock / Configuration / Entities / Facts / Projection / Validation / Validators
  git worktree remove /tmp/oracle                                  # 読み終わったら
  ```

- **適用範囲**: 移植完走時点（ゴールデンの出所 = HandballRecorder main `b7cf57e`）の移植面に限る。完走後に Rust コアへ独自追加された挙動は凍結オラクルに存在せず、不一致は退行ではない。これらは **Rust 実装 + 該当 ADR の「実装追記」が正**:
  - 記録オフセットが phase 境界 / stoppage 区間を越えないクランプ（`72c1024` / handball-project#92）
  - 非有限 anchor（NaN / ±∞）の validation（`8208d35` / handball-project#91）
  - 試合全体を覆っているかの coverage 検査（`1b2ac7d` / handball-project#90）
  - サンプル試合 import の atomic 化（`0f2b90d` / handball-project#83）
  - `AvailableActions.can_record_free_note` を R7 / R8 に合わせて `Playing` のみ true に変更（handball-project#177。ゴールデン期待値も追随済み）

- **移植面のセマンティクスに迷ったら**、凍結オラクルとそのテスト（約 2,500 行）を読む。型仕様・validation ルールは HandballRecorder main に残っている `docs/redesign/DOMAIN_TYPES_V1.md` / `DOMAIN_VALIDATION_RULES.md`（削除されていないので tag 不要）、ドメイン語彙は同リポの `CONTEXT.md`
- **パリティ検証**: [handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) の実試合 JSON をゴールデンコーパスに、Swift 実装をオラクルとして projection 出力の一致を検証する（`crates/handball-toolkit/tests/golden/`。期待値は dump 済みで、オラクルを再実行しなくても回る）。特に `SegmentResolver` と validation R3–R9 は移植で最も繊細な部分 — 移植面については挙動を「改善」せず一致させる
