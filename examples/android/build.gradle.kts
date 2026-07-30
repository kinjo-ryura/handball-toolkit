// ルートビルドスクリプト。プラグイン版は app モジュール側で適用する。
//
// バージョンは Gradle（flake の pkgs.gradle）と互換な組み合わせで固定する。
// 上げるときは README「バージョンの対応関係」も同時に直すこと。
plugins {
    id("com.android.application") version "8.11.1" apply false
    id("org.jetbrains.kotlin.android") version "2.1.21" apply false
    id("com.google.devtools.ksp") version "2.1.21-2.0.1" apply false
}
