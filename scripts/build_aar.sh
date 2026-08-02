#!/usr/bin/env bash
# UniFFI → Android 配布物（.aar）のビルド（handball-project#135。配布境界は ADR 0006）。
#
# 作るのは **外部利用者へ配る 1 ファイル**。中身は「コンパイル済み Kotlin +
# jniLibs/arm64-v8a/*.so + 依存宣言（JNA / coroutines）+ consumer ProGuard ルール」で、
# 利用者は Rust も Nix も NDK も要らなくなる。
#
# 旧 scripts/build_android.sh（サンプルへ .so と生成 Kotlin を直接配置していた）は
# #135 で役割を終えたため削除した。サンプルは publish 済みの .aar を引く側に回っている。
#
# 成果物（コミットしない — ADR 0004 決定 8 のバイナリ非コミット方針を踏襲）:
#   - target/aar/handball-toolkit-<version>.aar
#
# 配布は別手順（README「リリース」節）:
#   本スクリプトを通したあと `gh release create v<version> target/aar/*.aar`。
#   配布先に Maven Central を採らなかった理由は ADR 0006 実装追記 2026-08-02。
#
# 前提:
#   - nix develop（または direnv）環境内で実行する
#   - ホストが ANDROID_NDK_ROOT / ANDROID_HOME を提供している（ADR 0006 決定 1）
set -euo pipefail
cd "$(dirname "$0")/.."

readonly TARGET=aarch64-linux-android
readonly ABI=arm64-v8a
readonly LIB=libhandball_toolkit_ffi.so
readonly MODULE=android/toolkit
readonly JNI_DIR="$MODULE/src/main/jniLibs/$ABI"
readonly GEN_DIR="$MODULE/src/generated/kotlin"
readonly OUT=target/aar

if [ -z "${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER:-}" ]; then
  cat >&2 <<'MSG'
error: クロスリンカが未設定です（CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER）。

  ADR 0006 決定 1 のとおり NDK はホスト環境が提供します。ANDROID_NDK_ROOT を
  設定した上で nix develop に入り直してください（flake の shellHook が拾います）。
MSG
  exit 1
fi

if [ -z "${ANDROID_HOME:-}" ]; then
  cat >&2 <<'MSG'
error: Android SDK の場所が未設定です（ANDROID_HOME）。

  Gradle が SDK（platforms / build-tools）を引くために必要です。NDK と同じく
  ホスト環境が提供します（ADR 0006 実装追記 2026-07-28）。
MSG
  exit 1
fi

# 配布物のバージョンはコア crate に従う。ずれると「配布された .aar がどのコアか」が
# 追えなくなるため、ビルド前に照合して不一致なら止める（README「バージョンの対応関係」）。
cargo_version=$(grep -m1 '^version = ' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
gradle_version=$(grep -m1 'val toolkitVersion = ' "$MODULE/build.gradle.kts" | sed 's/.*"\(.*\)".*/\1/')
if [ "$cargo_version" != "$gradle_version" ]; then
  cat >&2 <<MSG
error: バージョンが一致しません。

  Cargo.toml [workspace.package]     : $cargo_version
  $MODULE/build.gradle.kts (toolkitVersion): $gradle_version

  配布物のバージョンはコア crate の version に従います。両方を揃えてください。
MSG
  exit 1
fi
echo "==> バージョン ${cargo_version}（Cargo.toml と Gradle が一致）"

echo "==> 1/4 $TARGET の共有ライブラリをビルド"
cargo build --release -p handball-toolkit-ffi --target "$TARGET"

echo "==> 2/4 .so を jniLibs へ配置"
rm -rf "$MODULE/src/main/jniLibs"
mkdir -p "$JNI_DIR"
cp "target/$TARGET/release/$LIB" "$JNI_DIR/$LIB"

echo "==> 3/4 Kotlin バインディング生成（library mode: .so の uniffi メタデータから）"
# --no-format: 整形は ktlint 任せだが devShell に入れていない（生成物はコミットしないため
# 差分の読みやすさが要らない）。付けないと毎回 ktlint 不在の警告が出る。
rm -rf "$GEN_DIR"
cargo run -q -p handball-toolkit-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "target/$TARGET/release/$LIB" \
  --language kotlin --out-dir "$GEN_DIR" --no-format

echo "==> 4/4 .aar を組み立て"
gradle -p android --quiet :toolkit:assembleRelease

rm -rf "$OUT"
mkdir -p "$OUT"
cp "$MODULE/build/outputs/aar/toolkit-release.aar" "$OUT/handball-toolkit-$cargo_version.aar"

echo "==> 完了: $OUT"
ls -lh "$OUT"
echo
echo "同梱物:"
unzip -l "$OUT/handball-toolkit-$cargo_version.aar" | grep -E "classes.jar|\.so|AndroidManifest|proguard"
