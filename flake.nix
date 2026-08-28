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

        # Keep the Nix toolchain aligned with Cargo.toml's minimum supported
        # Rust version so source builds behave consistently across installers.
        rustToolchain = pkgs.rust-bin.stable."1.90.0".default;

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

        concord = pkgs.callPackage ./nix/package.nix {
          inherit craneLib;
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          description = cargoToml.package.description;
          homepage = cargoToml.package.homepage;
          src = craneLib.cleanCargoSource ./.;
        };
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
