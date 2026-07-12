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
        packages = [ toolchain ];
      };
    };
}
