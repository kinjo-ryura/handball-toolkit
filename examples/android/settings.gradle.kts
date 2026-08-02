pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        // publish 前 / コア変更後のローカル検証用（handball-project#135）。
        //   ./scripts/build_aar.sh && gradle -p android :toolkit:publishToMavenLocal
        // で ~/.m2 に入ったものをここから引く。**Maven Central へ実際に上がった成果物を
        // 検証したいときは、この行をコメントアウトする**（ローカルの同一バージョンが
        // 勝ってしまい、公開物の検証にならないため）。
        mavenLocal()
        google()
        mavenCentral()
    }
}

rootProject.name = "handball-toolkit-android-sample"
include(":app")
