{
  description = "Configurable jj subcommand for GitHub PR workflows";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      crane,
      flake-utils,
      nixpkgs,
      treefmt-nix,
    }:
    flake-utils.lib.eachSystem [ "aarch64-darwin" "aarch64-linux" "x86_64-linux" ] (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        src =
          let
            root = ./.;
          in
          pkgs.lib.fileset.toSource {
            inherit root;
            fileset = pkgs.lib.fileset.unions [
              (craneLib.fileset.commonCargoSources root)
              (pkgs.lib.fileset.fileFilter (f: f.hasExt "gql") root)
              (pkgs.lib.fileset.fileFilter (f: f.hasExt "graphql") root)
            ];
          };
        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs =
            (pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.libiconv
            ])
            ++ (pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
              pkgs.openssl
            ]);
          nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.pkg-config ];
        };
        # Explicitly workspace-wide. The root manifest has a `[package]`
        # section, so a bare `cargo build` would select only `jj-gh` and the
        # artifacts would miss deps that the devtool packages need. A superset
        # is safe here: `--workspace` resolves the same features as every
        # narrower selection used below, it just covers more crates.
        cargoArtifacts = craneLib.buildDepsOnly (
          commonArgs // { cargoExtraArgs = "--locked --workspace"; }
        );
        treefmtEval = treefmt-nix.lib.evalModule pkgs (
          import ./nix/treefmt.nix {
            inherit pkgs;
          }
        );
        packageSet = import ./nix/pkgs {
          inherit
            pkgs
            craneLib
            commonArgs
            cargoArtifacts
            treefmtEval
            ;
        };
        inherit (packageSet)
          fetch-schema-app
          jj-gh
          release-app
          update-docs
          ;
      in
      {
        inherit (packageSet) packages;
        apps = {
          default = flake-utils.lib.mkApp { drv = jj-gh; };
          docs = flake-utils.lib.mkApp {
            drv = update-docs;
            name = "update-docs";
          };
          release = flake-utils.lib.mkApp {
            drv = release-app;
            name = "release";
          };
          fetch-schema = flake-utils.lib.mkApp {
            drv = fetch-schema-app;
            name = "fetch-schema";
          };
        };
        formatter = treefmtEval.config.build.wrapper;
        devShells.default = craneLib.devShell {
          inputsFrom = [ jj-gh ];
          packages = [
            pkgs.cargo-nextest
            pkgs.cargo-semver-checks
            pkgs.cargo-machete
            pkgs.ast-grep
            pkgs.jujutsu
            pkgs.rust-analyzer
            treefmtEval.config.build.wrapper
          ];
        };
        checks = import ./nix/checks {
          inherit
            self
            pkgs
            craneLib
            commonArgs
            cargoArtifacts
            treefmtEval
            jj-gh
            update-docs
            ;
        };
      }
    )
    // {
      overlays.default = final: _prev: {
        jj-gh = self.packages.${final.stdenv.hostPlatform.system}.default;
      };
      homeManagerModules.default = import ./nix/hm-module.nix self;
      homeManagerModules.jj-gh = import ./nix/hm-module.nix self;
    };
}
