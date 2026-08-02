# 消費側アプリが R8 で minify するときに適用されるルール（.aar に同梱され、利用者が
# 何も書かなくても効く）。
#
# UniFFI 生成コードは JNA の direct mapping（Native.register）で .so のシンボルを解決し、
# JNA は実行時に reflection でクラス・フィールドを引く。R8 がこれらを削る / 改名すると
# UnsatisfiedLinkError や Structure のフィールド不一致になる。
# サンプル（examples/android）は isMinifyEnabled = false なので、この問題はサンプルでは
# 露見しない — 外部利用者の release ビルドで初めて出る種類の壊れ方。

# ── JNA 本体（公式推奨のルール）──
-dontwarn java.awt.*
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }

# ── UniFFI 生成コード ──
# JNA の Structure サブクラス（RustBuffer / RustCallStatus 等）と native 宣言を含むため、
# パッケージごと keep する。FFI 境界のライブラリなので、難読化の利益より壊れないことを
# 優先する。パッケージ名は uniffi.toml の package_name と一致させること。
-keep class io.github.kinjoryura.handballtoolkit.** { *; }
