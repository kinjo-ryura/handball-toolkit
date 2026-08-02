{
  description = "handball-toolkit の Rust 開発環境";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      system = "aarch64-darwin";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };

      # rust-overlay のツールチェーンは Nix の clang-wrapper を無条件に propagate する
      # （lib/mk-aggregated.nix の depsHostHostPropagated / propagatedBuildInputs）。
      # ここでは Nix の clang / apple-sdk を環境に入れず、リンクを Xcode CLT の
      # /usr/bin/cc に任せたいので propagation を空にする
      # （将来の iOS ターゲット / UniFFI → XCFramework ビルドで xcrun 系と衝突させないため）。
      toolchain =
        (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).overrideAttrs {
          depsHostHostPropagated = [ ];
          propagatedBuildInputs = [ ];
          depsTargetTargetPropagated = [ ];
        };
    in
    {
      # mkShellNoCC: シェル自体にも Nix の cc を入れない。
      devShells.${system}.default = pkgs.mkShellNoCC {
        packages = [
          toolchain
          # wasm バインディングの JS グルー生成（handball-project#57）。
          # crates/handball-toolkit-wasm の wasm-bindgen 依存と**バージョン完全一致**が要る
          # （不一致だと生成時に schema version mismatch で落ちる）。nixpkgs 側が上がったら
          # Cargo.toml の `=` ピンも同時に合わせること。
          pkgs.wasm-bindgen-cli

          # Android サンプルシェル（examples/android）のビルド（handball-project#133）。
          # SDK / NDK をホストに任せる判断（ADR 0006 決定 1）は変えないが、gradle は
          # ビルドツールなので wasm-bindgen-cli と同格でこの flake が宣言する
          # （closure は約 200 MB で、SDK の 10.9 GiB とは桁が違う）。
          # JDK は gradle が自前で wrap したものを使うため別途入れない。
          # Gradle 自身のバージョンは AGP の要求と対応する — examples/android/README.md 参照。
          pkgs.gradle

          # 依存ライセンス一覧の生成（handball-project#140。scripts/generate_licenses.sh）。
          # 配布バイナリの OSS ライセンス表示は手書きせず Cargo.lock から起こす。
          # cargo-about が収集、jq が各シェル向けの JSON へ整形する。
          pkgs.cargo-about
          pkgs.jq
        ];

        # Android クロスリンク（handball-project#106）。
        #
        # SDK / NDK はこの flake では持たず、ホスト（dotfiles の nix-darwin）が提供する。
        # Android 環境は他プロジェクトでも使うためグローバル導入とし、ここは「あれば使う」に
        # 留める — ANDROID_NDK_ROOT が無い環境でも Android 以外のビルドは一切影響を受けない。
        #
        # linker を .cargo/config.toml に直書きしないのは、NDK の実体が nix store の
        # ユーザー固有絶対パスで、git 管理下に置くと OSS 公開（handball-project#134）で
        # 持ち出せず、かつ `nix flake update` のたびに壊れるため。
        #
        # ここで NDK clang を PATH に出さないことも重要: ホストリンクは Xcode CLT の
        # /usr/bin/cc に任せる方針（上の overrideAttrs のコメント参照）なので、
        # クロスリンカはフルパスで名指しし、PATH は汚さない。
        shellHook = ''
          if [ -n "''${ANDROID_NDK_ROOT:-}" ]; then
            # ホストディレクトリ名は Apple Silicon でも darwin-x86_64
            # （中身は universal binary で Rosetta は不要）。
            # API 24 は Android シェルの minSdk 暫定値 — 確定は handball-project#133。
            _hbt_android_cc="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android24-clang"
            if [ -x "$_hbt_android_cc" ]; then
              export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$_hbt_android_cc"
              # cc crate を引く依存が現れた場合に同じ clang を使わせる。
              export CC_aarch64_linux_android="$_hbt_android_cc"
            else
              echo "warn: ANDROID_NDK_ROOT は設定されているが NDK clang が見つかりません: $_hbt_android_cc" >&2
            fi
            unset _hbt_android_cc
          fi
        '';
      };
    };
}
