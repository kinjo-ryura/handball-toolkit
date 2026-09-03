// ルートビルドスクリプト。プラグイン版は app モジュール側で適用する。
//
// バージョンは Gradle（flake の pkgs.gradle）と互換な組み合わせで固定する。
// 上げるときは README「バージョンの対応関係」も同時に直すこと。
plugins {
    id("com.android.application") version "9.4.0" apply false
    id("org.jetbrains.kotlin.android") version "2.4.10" apply false
    id("com.google.devtools.ksp") version "2.3.11" apply false
}
