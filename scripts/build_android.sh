#!/usr/bin/env bash
# UniFFI → Android サンプルシェル向けの .so + Kotlin バインディング生成
# （ADR 0006 の配布経路。サンプル本体は examples/android — handball-project#133）。
#
# 成果物（いずれもコミットしない — ADR 0004 決定 8 のバイナリ非コミット方針を踏襲）:
#   - examples/android/app/src/main/jniLibs/arm64-v8a/libhandball_toolkit_ffi.so
#       JNA が実行時に dlopen する共有ライブラリ。**strip しない**（native crash の
#       スタックを辿れるようにするため。panic=abort 構成では panic が abort になるので、
#       シンボルが残っているかが診断可否を左右する — ADR 0006 決定 4 の再検討材料）
#   - examples/android/app/src/generated/kotlin/uniffi/handball_toolkit/handball_toolkit.kt
#       生成 Kotlin API 層（利用側がソースとして一緒にコンパイルする。Swift 側と同じ 2 段構え）
#
# 前提:
#   - nix develop（または direnv）環境内で実行する
#   - ホストが ANDROID_NDK_ROOT を提供している（ADR 0006 決定 1）。flake の shellHook が
#     そこから CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER を導出する
set -euo pipefail
cd "$(dirname "$0")/.."

readonly TARGET=aarch64-linux-android
readonly ABI=arm64-v8a
readonly LIB=libhandball_toolkit_ffi.so
readonly APP=examples/android/app
readonly JNI_DIR="$APP/src/main/jniLibs/$ABI"
readonly GEN_DIR="$APP/src/generated/kotlin"

if [ -z "${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER:-}" ]; then
  cat >&2 <<'MSG'
error: クロスリンカが未設定です（CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER）。

  ADR 0006 決定 1 のとおり NDK はホスト環境が提供します。ANDROID_NDK_ROOT を
  設定した上で nix develop に入り直してください（flake の shellHook が拾います）。
MSG
  exit 1
fi

echo "==> 1/3 $TARGET の共有ライブラリをビルド"
cargo build --release -p handball-toolkit-ffi --target "$TARGET"

echo "==> 2/3 .so を jniLibs へ配置"
mkdir -p "$JNI_DIR"
cp "target/$TARGET/release/$LIB" "$JNI_DIR/$LIB"

echo "==> 3/3 Kotlin バインディング生成（library mode: .so の uniffi メタデータから）"
# --no-format: 整形は ktlint 任せだが devShell に入れていない（生成物はコミットしないため
# 差分の読みやすさが要らない）。付けないと毎回 ktlint 不在の警告が出る。
rm -rf "$GEN_DIR"
cargo run -q -p handball-toolkit-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "target/$TARGET/release/$LIB" \
  --language kotlin --out-dir "$GEN_DIR" --no-format

echo "完了: $JNI_DIR/$LIB + $GEN_DIR"
