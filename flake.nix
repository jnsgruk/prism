{
  description = "Prism — engineering insights platform";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";

    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";

    git-hooks.url = "github:cachix/git-hooks.nix";
    git-hooks.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-parts,
      ...
    }@inputs:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks.flakeModule
      ];

      perSystem =
        { config, system, ... }:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs { inherit system overlays; };
          inherit (pkgs) lib;

          rust = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "rust-src"
              "clippy"
              "rust-analyzer"
              "rustfmt"
            ];
          };

          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rust;

          cargoSource = craneLib.cleanCargoSource ./.;

          src = lib.cleanSourceWith {
            src = lib.cleanSource ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              || (lib.hasInfix "/migrations" path)
              || (lib.hasInfix "/proto" path)
              || (lib.hasInfix "/.sqlx" path);
          };

          cargoToml = lib.trivial.importTOML ./Cargo.toml;

          commonArgs = {
            pname = "prism";
            version = cargoToml.workspace.package.version;

            nativeBuildInputs = with pkgs; [
              clang
              mold
              pkg-config
              protobuf
            ];

            buildInputs = with pkgs; [
              openssl
              stdenv.cc.cc.lib
            ];

            env = {
              LD_LIBRARY_PATH = lib.makeLibraryPath [ pkgs.openssl ];
              SQLX_OFFLINE = "true";
            };
          };

          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // { src = cargoSource; });
        in
        {
          packages = {
            default = self.packages.${system}.ps-server;

            ps-server = craneLib.buildPackage (
              commonArgs
              // {
                inherit src cargoArtifacts;
                cargoExtraArgs = "--bin ps-server";
                env = commonArgs.env // {
                  GIT_HASH = self.shortRev or self.dirtyShortRev or "dev";
                };
              }
            );

            ps-ingestion = craneLib.buildPackage (
              commonArgs
              // {
                inherit src cargoArtifacts;
                cargoExtraArgs = "--bin ps-ingestion";
              }
            );

            ps-migrate = craneLib.buildPackage (
              commonArgs
              // {
                inherit src cargoArtifacts;
                cargoExtraArgs = "--bin ps-migrate";
              }
            );

            psctl = craneLib.buildPackage (
              commonArgs
              // {
                inherit src cargoArtifacts;
                cargoExtraArgs = "--bin psctl";
              }
            );
          };

          devShells.default = pkgs.mkShell {
            name = "prism";

            NIX_CONFIG = "experimental-features = nix-command flakes";
            RUST_SRC_PATH = "${rust}/lib/rustlib/src/rust/library";
            LD_LIBRARY_PATH = lib.makeLibraryPath [ pkgs.openssl ];

            shellHook = ''
              ${config.pre-commit.shellHook}
            '';

            buildInputs = [
              rust
            ]
            ++ (with pkgs; [
              # Build tooling
              clang
              mold
              pkg-config
              openssl

              # Protobuf
              protobuf
              buf

              # Database
              sqlx-cli
              postgresql

              # Frontend
              bun
              typescript-go
              oxlint
              oxfmt

              # Nix tooling
              nil
              nixfmt

              # Dev tools
              cargo-watch
              tilt
              kubectl
              kubectx
              kubernetes-helm
              kustomize
            ])
            ++ config.pre-commit.settings.enabledPackages;
          };

          treefmt = {
            projectRootFile = "flake.nix";

            programs = {
              deadnix.enable = true;
              nixfmt.enable = true;
              rustfmt.enable = true;
              shfmt.enable = true;
            };

            settings.formatter = {
              oxfmt = {
                command = lib.getExe pkgs.oxfmt;
                includes = [
                  "*.ts"
                  "*.tsx"
                  "*.js"
                  "*.jsx"
                  "*.json"
                ];
              };
            };
          };

          pre-commit = {
            check.enable = false;
            settings = {
              package = pkgs.prek;
              hooks = {
                treefmt = {
                  enable = true;
                  package = config.treefmt.build.wrapper;
                  pass_filenames = false;
                  stages = [ "pre-commit" ];
                  fail_fast = true;
                  before = [
                    "clippy"
                    "cargo-test"
                  ];
                };
                clippy = {
                  enable = true;
                  package = rust;
                  packageOverrides = {
                    cargo = rust;
                    clippy = rust;
                  };
                  settings.extraArgs = "--allow-dirty --fix";
                  fail_fast = true;
                  before = [
                    "cargo-test"
                  ];
                };
                buf-lint = {
                  enable = true;
                  files = "\\.proto$";
                  entry = "buf lint";
                  pass_filenames = false;
                  stages = [ "pre-commit" ];
                  before = [
                    "clippy"
                    "cargo-test"
                  ];
                };
                frontend-lint = {
                  enable = true;
                  files = "\\.(ts|tsx|js|jsx)$";
                  entry = "bash -c 'cd frontend && bun run lint'";
                  pass_filenames = false;
                  stages = [ "pre-commit" ];
                  before = [
                    "clippy"
                    "cargo-test"
                  ];
                };
                cargo-test = {
                  enable = true;
                  files = "\\.(rs|toml)$";
                  entry = "cargo test";
                  pass_filenames = false;
                  stages = [ "pre-commit" ];
                };
              };
            };
          };
        };
    };
}
