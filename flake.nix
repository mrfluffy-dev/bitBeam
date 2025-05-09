{
  description = "A Rust project with development environment and build support";

  inputs = {
    #nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forEachSupportedSystem =
        f:
        nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            };
          }
        );
    in
    {
      # Define the package (your Rust binary)
      packages = forEachSupportedSystem (
        { pkgs }:
        {
          default = pkgs.rustPlatform.buildRustPackage {
            name = "bitBeam";
            src = ./.;

            # Specify dependencies (replace with your project's actual dependencies)
            buildInputs = [
              pkgs.openssl
              pkgs.pkg-config
            ];

            # Generate this with `cargo generate-lockfile` if you don't have it
            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            # Optional: Override the Rust version if needed
            nativeBuildInputs = [ pkgs.rust-bin.stable.latest.default ];
          };
        }
      );

      # Development environment (existing setup)
      devShells = forEachSupportedSystem (
        { pkgs }:
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rust-bin.stable.latest.default
              openssl
              pkg-config
              cargo-deny
              cargo-edit
              cargo-watch
              rust-analyzer
            ];
            RUST_SRC_PATH = "${pkgs.rust-bin.stable.latest.default}/lib/rustlib/src/rust/library";
            BITBEAM_DATABASE_URL = "sqlite://./bitbeam.sqlite";
            BITBEAM_DB_TYPE = "sqlite";
          };
        }
      );
    };
}
