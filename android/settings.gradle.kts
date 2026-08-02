// .aar 配布物のビルド専用プロジェクト（handball-project#135）。
//
// サンプルアプリ（examples/android）とは別の Gradle プロジェクトにしてある。配布する
// 成果物のソースがサンプルの中にあると、外部利用者から見て「どれが配布物で、どれが
// 参照実装か」が曖昧になるため。サンプルは publish 済みの .aar を引く側に回る。
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "handball-toolkit-android"
include(":toolkit")
