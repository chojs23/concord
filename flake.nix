{
  description = "Concord - a terminal user interface client for Discord";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Pin the Rust toolchain. Cargo.toml declares rust-version = "1.85",
        # but several transitive deps (quantette, ratatui, image, instability,
        # safe_arch, wide, time) require newer rustc, so the effective MSRV
        # is 1.90.
        rustToolchain = pkgs.rust-bin.stable."1.90.0".default;

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

        commonArgs = {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;

          src = craneLib.cleanCargoSource ./.;

          # zstd-sys builds the bundled C source by default, so it only needs
          # a C compiler from stdenv. pkg-config is included to keep the
          # build robust if a future dep grows a system-library probe.
          nativeBuildInputs = [ pkgs.pkg-config ];

          # Networking uses rustls + webpki-roots, so we do not need openssl
          # or a system CA bundle here. Only Darwin needs Security/CoreFoundation
          # frameworks because some indirect crates link against them.
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.alsa-lib
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.CoreFoundation
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];

          # The unit tests in this repo do not require network or a TTY, but
          # disable them by default to keep `nix build` fast and reproducible.
          # Run `cargo test` inside `nix develop` for the full test suite.
          doCheck = false;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        concord = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          meta = with pkgs.lib; {
            description = cargoToml.package.description;
            homepage = cargoToml.package.homepage;
            license = licenses.gpl3Only;
            mainProgram = "concord";
            platforms = platforms.unix;
          };
        });
      in
      {
        packages = {
          default = concord;
          concord = concord;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = concord;
          name = "concord";
        };

        devShells.default = craneLib.devShell {
          inputsFrom = [ concord ];
          packages = [
            rustToolchain
            pkgs.rust-analyzer
            pkgs.cargo-edit
          ];
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
