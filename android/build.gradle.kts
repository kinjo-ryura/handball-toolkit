// ルートビルドスクリプト。プラグイン版は toolkit モジュール側で適用する。
//
// AGP / Kotlin / KSP のバージョンは examples/android と揃える（同じ Gradle = flake の
// pkgs.gradle で動かすため）。上げるときは examples/android/build.gradle.kts と
// examples/android/README.md「バージョンの対応関係」も同時に直すこと。
plugins {
    id("com.android.library") version "8.11.1" apply false
    id("org.jetbrains.kotlin.android") version "2.1.21" apply false
}
