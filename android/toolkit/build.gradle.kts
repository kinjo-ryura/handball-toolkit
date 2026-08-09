plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// コア crate（ワークスペース Cargo.toml の workspace.package.version）と同じ値を置く。
// ずれると「配布された .aar がどのコアなのか」が追えなくなるため、scripts/build_aar.sh が
// ビルド前に Cargo.toml と照合して不一致なら止める。
val toolkitVersion = "0.1.0"

android {
    // 生成 Kotlin の package_name（crates/handball-toolkit/uniffi.toml）と揃える。
    namespace = "io.github.kinjoryura.handballtoolkit"
    compileSdk = 36

    // ライブラリのリソースは利用側アプリの名前空間へマージされるため、衝突しない
    // 接頭辞を強制する（付け忘れは lint が警告する）。文言リソースは
    // src/main/res/values*/strings.xml — handball-project#136。
    resourcePrefix = "handball_toolkit_"
    // nix が提供する SDK には build-tools が 1 つしか入っていないため明示する。既定値
    // （AGP のバンドル値）を要求されると read-only な nix store へダウンロードしようと
    // して失敗する（examples/android/app と同じ理由）。
    buildToolsVersion = "37.0.0"

    defaultConfig {
        // ADR 0006 実装追記（2026-07-28）で確定した値。NDK リンカの API レベルと一致する。
        minSdk = 24

        // ADR 0006 決定 5: arm64-v8a 単独。
        ndk { abiFilters += "arm64-v8a" }

        // 消費側が R8 で minify したときに JNA / 生成コードが削られないようにする。
        // .aar に proguard.txt として同梱され、利用者が何も書かなくても効く。
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        // 生成 Kotlin の API 面に java.time.Instant が出る（UtcDateTime）。java.time は
        // API 26 以降の標準ライブラリなので minSdk 24 では desugaring が要る。
        // **消費側アプリでも同じ設定が必要**（ライブラリ側の有効化は消費側に伝播しない）
        // — README の Android 節に利用者向けの注意として書いてある。
        isCoreLibraryDesugaringEnabled = true
    }

    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }

    // scripts/build_aar.sh が生成する Kotlin バインディング（生成物なのでコミットしない）。
    sourceSets["main"].kotlin.srcDir("src/generated/kotlin")
    // 手書きのシム層（handball-project#136）。生成物と混ざらないよう別ディレクトリに置く。
    sourceSets["main"].kotlin.srcDir("src/main/kotlin")

    packaging {
        jniLibs {
            // .so を strip しない。panic = "abort"（ADR 0006 決定 4）ではコアの panic が
            // Kotlin 例外ではなくネイティブ abort になるため、シンボルが残っているか
            // どうかがそのまま診断可否になる。サイズより診断性を採る。
            keepDebugSymbols += "**/*.so"
        }
    }
}

dependencies {
    // ── UniFFI 生成コードの実行時依存 ──
    // JNA: 生成コードが Native.register（direct mapping）で .so を dlopen する。
    // Android では aar 版（各 ABI のネイティブ支援ライブラリ同梱）が要る。
    // coroutines: 生成コードの suspend 関数・GlobalScope.launch が依存する。
    //
    // **GitHub Release 配布では、この 2 行は利用者側にも必要**（ADR 0006 実装追記
    // 2026-08-02）。.aar ファイル単体は依存情報を運ばない — 運ぶのは Maven の POM で、
    // Release に置いた .aar を implementation(files(...)) で参照する形では POM が
    // 介在しないため。README の Android 節に利用者向けのコピペ用として載せてある。
    // ここでの api 宣言はライブラリ自身のコンパイルに効き、将来 Maven publish へ
    // 格上げしたときはそのまま POM の compile scope に出る。
    api("net.java.dev.jna:jna:5.17.0@aar")
    api("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")

    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")

    // シムの単体テスト（handball-project#136）。JVM 上で回り、.so も端末も要らない
    // — シムは生成 data class を組み替えるだけでネイティブに触らないため。
    testImplementation(kotlin("test"))
}
