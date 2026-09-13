# CLAUDE.md

## リポジトリ概要

ハンドボール試合データのツールキット（Rust workspace）。[HandballRecorder](https://github.com/kinjo-ryura/HandballRecorder) のドメイン層 `RecorderDomain`（Swift の純粋計算）の移植であり、単一の共有コアを iOS / Android / Web (wasm) / CLI へ届けるための基盤。[handball-project](https://github.com/kinjo-ryura/handball-project) の submodule（`apps/handball-toolkit/`）として管理される。

- 設計の正典: `docs/adr/`（0001 境界 API / 0002 エラー体系 / 0003 パリティ検証 / 0004 iOS FFI 本境界 / 0005 write orchestration / 0006 Android 配布境界）。**各 ADR の「実装追記」が実装の現況を持つ**
- 境界のエラーコード一覧は [`docs/ERROR_CODES.md`](docs/ERROR_CODES.md)。**エラー case を追加・改名したらこの表も更新する**（code は安定契約 — ADR 0002 決定 2）。`DomainValidationMessagesTest` が表の 1 列目を sealed 型の case 名と集合比較するので、**期待する件数をテストへ書き写さないこと**。テストは表の書式に依存する — 1 列目の `` `code` `` と見出し末尾の `(N)` を崩さない
- [`docs/PORTING.md`](docs/PORTING.md) は移植の完了記録（現在地の管理台帳ではない）。進行中・未着手の作業は GitHub Issues が正
- ドキュメント・コードコメントは日本語で書く。**例外は [`docs/ERROR_CODES.md`](docs/ERROR_CODES.md) の 1 本のみ**（外部シェル実装者向けの参照表なので英語。翻訳の二重管理を作らない）

## 変更の出し方（main は保護されている）

**main へ直接 push できない。** ruleset [`protect-main`](https://github.com/kinjo-ryura/handball-toolkit/rules/19753789) が **直 push / force push / main の削除を禁止し、PR 必須 + CI の `check` ジョブ green 必須**にしている。bypass actor は無しなので**オーナーでも通らない**。docs 1 行の修正でもブランチを切る。

```bash
git switch -c docs/xxx                    # prefix は feat/ fix/ ci/ docs/ + 内容
git push -u origin HEAD
gh pr create --title "..." --body "..."   # 本文に関連 Issue（handball-project#NN）を書く
gh pr checks --watch                      # required = `check` ジョブのみ。macOS + Nix で 5〜7 分
gh pr merge --merge --delete-branch       # required approvals は 0 なので自分の PR を自分で merge できる
git switch main && git pull               # ローカル main を merge 後の状態へ追従させる
```

- **CI が落ちている PR は merge できない**。push 前にローカルで `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` を通す
- **親リポの submodule pointer は merge 後の main を指す**（PR ブランチの commit を直接指さない）。push 順は **toolkit → 親リポ** — 逆にすると親リポがリモートに無い commit を指す
- force push がどうしても必要になったときの退避策（ruleset の一時無効化）は [`docs/PORTING.md`](docs/PORTING.md)「作業規律」

## 開発コマンド

開発環境は Nix flake + direnv で宣言的に管理する（rustup 不使用）。ツールチェーンは `rust-toolchain.toml` でバージョン固定し、rust-overlay が提供する。

```bash
direnv allow          # 初回のみ。direnv を使わない場合は nix develop
cargo test            # 全テスト
cargo test <部分一致>  # 単一テスト
cargo clippy          # lint
cargo fmt             # フォーマット
```

- ツールチェーン更新は `rust-toolchain.toml` の `channel` を書き換えて `direnv reload`。クロスターゲットも同ファイルの `targets` に足す
- **flake.nix に Nix の clang / apple-sdk を入れないこと**: リンクは意図的に Xcode CLT の `/usr/bin/cc` に任せている（xcrun 系と衝突させないため）。rust-overlay の propagation を空にしている `overrideAttrs` を外さない（詳細は flake.nix のコメント）

## アーキテクチャ

Cargo workspace。4 crate 構成:

- `crates/handball-toolkit/` — コア crate（facts / clocks / configuration / entities / validators / projections）。feature `uniffi`（default off）で UniFFI derive と FFI 関数公開（`ffi_api` / `ffi_support`）が有効になる（ADR 0004。wasm / CLI ビルドでは uniffi が依存グラフごと消える）
- `crates/handball-toolkit-cli/` — sample-matches 配信 JSON（SAMPLE_DTO_V2）の検証 CLI。コアの validators を呼ぶだけの薄いシェル。使い方は README「検証 CLI」
- `crates/handball-toolkit-ffi/` — FFI パッケージング crate。staticlib 化と uniffi-bindgen CLI（feature `bindgen`）のみを担い、公開面はコア crate の namespace に集約する
- `crates/handball-toolkit-wasm/` — wasm パッケージング crate。JS 向けの粗粒度エントリとマーシャリングだけを担う。**ID 生成はシェル（JS の `crypto.randomUUID()`）が行う**（設計不変条件 2）ので、この crate も乱数を引かない

Kotlin バインディングは専用の workspace member を持たない。生成設定は `crates/handball-toolkit/uniffi.toml` の `[bindings.kotlin]`、`.so` と Kotlin は `scripts/build_aar.sh` が生成し、`android/toolkit/` の Gradle モジュールが `.aar` に束ねる（配布境界は ADR 0006）。

### iOS 向け XCFramework（UniFFI）

```bash
./scripts/build_xcframework.sh   # target/xcframework/ に HandballToolkit.xcframework + 生成 Swift API 層
./scripts/ios_poc/run.sh         # 本境界 smoke をビルドして iOS シミュレータ内で実行
```

XCFramework は ios / ios-sim / macos の 3 スライス。**生成 Swift（`HandballToolkit.swift`）は XCFramework に入らない** — 利用側がソースとして一緒にコンパイルする（UniFFI の標準配布形）。

### Web 向け wasm（wasm-bindgen）

```bash
./scripts/build_wasm.sh   # target/wasm/ に .wasm + ES module の JS グルー + .d.ts
```

`wasm-bindgen` crate と `wasm-bindgen-cli`（flake が nixpkgs から入れる）は**バージョン完全一致**が必要。Cargo.toml 側は `=` でピン留めしてあるので、nixpkgs が上がったら両方を同時に合わせる（不一致は生成時の schema version mismatch で落ちる）。

### Android 向け `.aar`（UniFFI + JNA）

```bash
./scripts/build_aar.sh   # → target/aar/handball-toolkit-<version>.aar
```

配布物は **GitHub Release に添付する prebuilt `.aar`**（Maven Central を採らなかった理由は ADR 0006 実装追記）。利用者は Rust / Nix / NDK を要しない。リリース手順は README「リリース」。

- **シムと文言リソース**（`android/toolkit/src/main/kotlin/` と `src/main/res/values*/`）は手書き。生成物の `src/generated/kotlin/` は `build_aar.sh` が毎回消して作り直す
- **FFI 公開面か `.aar` 同梱物を変えたら、その PR を merge した後に次の版を切る**。対象は ① `ffi_api` / `ffi_support` の関数・型の増減・改名・挙動変更 ② `.aar` 同梱物（生成 Kotlin / シム / 文言リソース / ライセンス JSON / `.so`）の変化。放置すると README の案内と実配布物が黙って食い違う。**CHANGELOG ファイルは置かない。変更履歴は Release notes が正**
- NDK / SDK は**この repo の flake ではなくホスト環境**が提供する（ADR 0006 決定 1）。`ANDROID_NDK_ROOT` があれば shellHook がクロスリンカを export する。未設定なら Android ターゲットだけがビルドできない
- ABI は `arm64-v8a` 単独（ADR 0006 決定 5）。`.so` の実行時依存は `libdl.so` / `libc.so` のみで、strip しない（ADR 0006 決定 4）
- 生成 Kotlin の package は `io.github.kinjoryura.handballtoolkit`（ADR 0006 決定 6）
- **NDK clang を PATH に出さないこと**: ホストリンクは Xcode CLT の `/usr/bin/cc` に任せる。クロスリンカはフルパスで名指しする
- **エラー型のフィールドに `message` という名前を使わないこと**: Kotlin backend の error 型が `Throwable.message` と衝突してコンパイルできない（Swift では露見しない。診断文字列は `detail` に統一）
- **consumer ProGuard ルールを消さないこと**（`android/toolkit/consumer-rules.pro`）: JNA は reflection で引くため、消費側が R8 で minify すると壊れる。サンプルは minify しないので**サンプルでは露見しない**
- **依存を増減したら 3 箇所を揃える**（`android/toolkit/build.gradle.kts` / README / `examples/android/app/build.gradle.kts`）。`.aar` 単体は依存情報を運ばないので、JNA と kotlinx-coroutines は利用側が自分で宣言する
- **validation / write の case を増やしたら文言を 2 ロケール分足すこと**（`src/main/res/values/` と `values-ja/`）。`values-ja` の漏れはコンパイラに見えない。`DomainValidationMessagesTest`（CI でも走る）が文言 2 ロケールと `docs/ERROR_CODES.md` の表の 3 箇所が揃うまで赤くする
- **リソース名には `handball_toolkit_` 接頭辞を付けること**（`resourcePrefix` が lint で見張る）
- **シムに探索やドメイン規則を書かないこと**: 許可されるのは「self のみ / ループ・再帰・探索なし / ドメイン規則を含まない」の 3 条件を満たすものだけ（ADR 0004 決定 4）。半開区間・優先順位・丸め・閾値に触れる計算はコアに置く

### Android サンプルシェル（`examples/android/`）

Room + 3 trait の 15 メソッド + 最小 UI の参照実装。**公開できるシェル実装はこれだけ**。ビルド手順と Android 固有の落とし穴は [`examples/android/README.md`](examples/android/README.md)。

**このサンプルは配布された `.aar` を `app/libs/` から参照する**（外部利用者と同じ経路）。コアを直したら `./scripts/build_aar.sh` の出力を `examples/android/app/libs/` へコピーする。

### 設計不変条件（コアに入れてよいもの / いけないもの）

1. **状態を所有しない stateless コア** — コアは DB ハンドル・保存実体・UI 状態を所有しない。判断・計画（何をどの順に保存すべきか）は「fact 列 in → 導出結果 out」の純粋関数として置く。ただし**永続化の発火 orchestration**（注入された repository を await する薄い export 関数）は feature `uniffi` 配下の境界層として持てる（ADR 0005）。repository を保持する long-lived object は作らない
2. **決定性** — `now()` / UUID 生成をコアに置かない。timestamp / ID はシェルが発行して fact に載せて渡す（ゴールデンテストの安定と wasm 対応のため）
3. **エラーは構造化** — エラーコード + パラメータのみを返す。ユーザー向け文言をコアに焼き込まない（文言は各シェルが持つ）。移植元の `DomainValidationMessage` をそのまま写さないこと — ここは意図的な再設計ポイント
4. **境界は粗い粒度** — FFI / JNI / wasm 越えを前提に、細かい getter の応酬ではなく「fact 列 in → projection out」の同期バッチ形状を保つ

### 移植のオラクル（Swift 実装）とパリティ検証

**オラクルは凍結済み。「Swift が真実の仕様」は移植面にのみ適用される** — この 2 点を取り違えると、Rust 独自に進化した挙動を「オラクルと不一致だから」と誤って巻き戻す。

- **オラクルの現在地**: RecorderDomain は HandballRecorder main から削除済み（`8aeffb8`）。到達手段は tag `oracle-dump-final` からの取り出しのみ:

  ```bash
  # ../HandballRecorder/ で。main を汚さずに読むため worktree を使う
  git worktree add /tmp/oracle oracle-dump-final
  ls /tmp/oracle/Packages/RecorderDomain/Sources/RecorderDomain/
  git worktree remove /tmp/oracle                                  # 読み終わったら
  ```

- **適用範囲**: 移植完走時点（ゴールデンの出所 = HandballRecorder main `b7cf57e`）の移植面に限る。完走後に Rust コアへ独自追加された挙動は凍結オラクルに存在せず、不一致は退行ではない。これらは **Rust 実装 + 該当 ADR の「実装追記」が正**:
  - 記録オフセットが phase 境界 / stoppage 区間を越えないクランプ（`72c1024` / handball-project#92）
  - 非有限 anchor（NaN / ±∞）の validation（`8208d35` / handball-project#91）
  - 試合全体を覆っているかの coverage 検査（`1b2ac7d` / handball-project#90）
  - サンプル試合 import の atomic 化（`0f2b90d` / handball-project#83）
  - `AvailableActions.can_record_free_note` を R7 / R8 に合わせて `Playing` のみ true に変更（handball-project#177）
  - `LiveMatchProjection::build_timer_mode` / `build_highlight_mode`（handball-project#354。オラクルは `build_video_mode` しか持たない）

- **移植面のセマンティクスに迷ったら**、凍結オラクルとそのテストを読む。いずれも HandballRecorder の checkout を要し、このリポ単体からは辿れない — 外部の読み手に向けた正典は [`docs/ERROR_CODES.md`](docs/ERROR_CODES.md) と `docs/adr/`、コーパスの schema は [handball-sample-matches の `v2/SCHEMA.md`](https://github.com/kinjo-ryura/handball-sample-matches/blob/main/v2/SCHEMA.md)
- **パリティ検証**: [handball-sample-matches](https://github.com/kinjo-ryura/handball-sample-matches) の実試合 JSON をゴールデンコーパスに、Swift 実装をオラクルとして projection 出力の一致を検証する（`crates/handball-toolkit/tests/golden/`。期待値は dump 済み）。特に `SegmentResolver` と validation R3–R9 は移植で最も繊細な部分 — 移植面については挙動を「改善」せず一致させる
