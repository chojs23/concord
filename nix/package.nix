{ lib
, stdenv
, craneLib
, cmake
, rustPlatform
, pkg-config
, makeWrapper
, nasm
, alsa-lib
, pipewire
, libva
, mesa
, src
, pname
, version
, description
, homepage
,
}:

let
  baseArgs = {
    inherit pname version src;

    cargoExtraArgs = "--locked";

    # PipeWire generates bindings at build time. The bindgen hook supplies
    # libclang and its search path without hard-coding a Nix store location.
    # OpenH264 uses NASM on x86_64, while AArch64 builds its NEON sources with cc.
    nativeBuildInputs = [
      cmake
      pkg-config
    ] ++ lib.optionals stdenv.isLinux [
      rustPlatform.bindgenHook
      makeWrapper
    ] ++ lib.optionals stdenv.hostPlatform.isx86_64 [
      nasm
    ];

    # Networking uses rustls + webpki-roots, so we do not need openssl or a
    # system CA bundle here. Darwin stdenv provides the SDK by default, so avoid
    # legacy darwin.apple_sdk framework stubs.
    buildInputs = lib.optionals stdenv.isLinux [
      alsa-lib
      pipewire
      libva
      mesa
    ];

    # The unit tests in this repo do not require network or a TTY, but disable
    # them by default to keep `nix build` fast and reproducible. Run `cargo test`
    # inside `nix develop` for the full test suite.
    doCheck = false;

  };

  # PipeWire's sys crates ask bindgen to evaluate cast macros such as
  # SPA_ID_INVALID and PW_ID_ANY. Bindgen otherwise writes its fallback files
  # beside the vendored crate, which crane keeps in the immutable Nix store.
  # TODO: Remove these patches after the sys crates set a fallback build directory.
  cargoVendorDir = craneLib.vendorCargoDeps (baseArgs // {
    overrideVendorCargoPackage = package: crate:
      if package.name == "libspa-sys" && package.version == "0.10.0" then
        crate.overrideAttrs
          (old: {
            patches = (old.patches or [ ]) ++ [
              ./patches/libspa-sys-0.10.0-bindgen-out-dir.patch
            ];
          })
      else if package.name == "pipewire-sys" && package.version == "0.10.0" then
        crate.overrideAttrs
          (old: {
            patches = (old.patches or [ ]) ++ [
              ./patches/pipewire-sys-0.10.0-bindgen-out-dir.patch
            ];
          })
      else
        crate;
  });

  commonArgs = baseArgs // {
    inherit cargoVendorDir;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (commonArgs // {
  inherit cargoArtifacts;

  # buildDepsOnly has no installed binary, so apply the runtime wrapper only
  # to the final package output.
  postFixup = lib.optionalString stdenv.isLinux ''
    wrapProgram "$out/bin/concord" \
      --set-default PIPEWIRE_CONFIG_DIR "${pipewire}/share/pipewire"
  '';

  meta = {
    inherit description homepage;
    license = lib.licenses.gpl3Only;
    mainProgram = "concord";
    platforms = lib.platforms.unix;
  };
})
