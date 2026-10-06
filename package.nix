{
  lib,
  stdenv,
  runCommand,
  rustPlatform,
  rustTool,
  microkitManifest,
  pkgsCross,
  python312,
  cmake,
  ninja,
  dtc,
  libxml2,
  qemu,
  pandoc,
  texlive,
  makeWrapper,
  nukeReferences,
  # Empty lists select all upstream boards/configurations.
  boards ? [ ],
  configs ? [ ],
}:

let
  sel4RevisionFile = runCommand "microkit-sel4-revision" { nativeBuildInputs = [ libxml2 ]; } ''
    xmllint --nonet --xpath 'string(/manifest/project[@name="seL4"]/@revision)' \
      ${microkitManifest}/main.xml > "$out"
  '';

  sel4Revision = lib.trim (builtins.readFile sel4RevisionFile);

  sel4Src = builtins.fetchTree {
    type = "github";
    owner = "seL4";
    repo = "seL4";
    rev = sel4Revision;
  };

  python = python312.withPackages (
    ps: with ps; [
      ply
      jinja2
      pyaml
      lxml
      pyfdt
      setuptools
      jsonschema
      autopep8
    ]
  );

  tex = texlive.combine {
    inherit (texlive)
      scheme-small
      fancyvrb
      parskip
      titlesec
      enumitem
      sfmath
      roboto
      fontaxes
      isodate
      substr
      tcolorbox
      environ
      pdfcol
      ;
  };

  crossCompilers = {
    aarch64 = pkgsCross.aarch64-embedded.stdenv.cc;
    riscv64 = pkgsCross.riscv64-embedded.stdenv.cc;
    x86_64 = pkgsCross.x86_64-embedded.stdenv.cc;
  };

  sdkFlags = [
    "--sel4=seL4"
    "--skip-tar"
    "--tool-target-triple=${stdenv.hostPlatform.rust.rustcTarget}"
  ]
  ++ lib.mapAttrsToList (
    arch: cc: "--gcc-toolchain-prefix-${arch}=${lib.removeSuffix "-" cc.targetPrefix}"
  ) crossCompilers
  ++ lib.optional (boards != [ ]) "--boards=${lib.concatStringsSep "," boards}"
  ++ lib.optional (configs != [ ]) "--configs=${lib.concatStringsSep "," configs}";
in
stdenv.mkDerivation (finalAttrs: {
  pname = "microkit-sdk";
  version = lib.trim (builtins.readFile ./VERSION);

  src = lib.cleanSource ./.;

  # out: complete SDK, doc: manual.
  outputs = [
    "out"
    "doc"
  ];

  cargoDeps = rustPlatform.importCargoLock {
    lockFile = ./Cargo.lock;
    allowBuiltinFetchGit = true;
  };

  nativeBuildInputs = [
    rustTool
    rustPlatform.cargoSetupHook
    rustPlatform.bindgenHook
    python
    cmake
    ninja
    dtc
    libxml2
    qemu
    pandoc
    tex
    makeWrapper
    nukeReferences
  ]
  ++ lib.concatMap (cc: [
    cc.cc
    cc.bintools.bintools
  ]) (lib.attrValues crossCompilers);

  strictDeps = true;
  dontUseCmakeConfigure = true;
  # Preserve DWARF and the ELF symbols used by the image builder.
  dontStrip = true;
  dontPatchELF = true;

  # Keep Rust source paths from retaining the host toolchain.
  env."CARGO_TARGET_${stdenv.hostPlatform.rust.cargoEnvVarTarget}_RUSTFLAGS" =
    "--remap-path-prefix=${rustTool}=/rustc";

  postPatch = ''
    cp -r ${sel4Src} seL4
    chmod -R u+w seL4
    patchShebangs seL4
  '';

  buildPhase = ''
    runHook preBuild

    python3 build_sdk.py ${lib.escapeShellArgs sdkFlags}

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall

    sdk="release/microkit-sdk-${finalAttrs.version}"
    mkdir -p "$out/bin" "$out/lib"

    mv "$sdk/doc" "$doc"
    mv "$sdk" "$out/lib/microkit"

    makeWrapper "$out/lib/microkit/bin/microkit" "$out/bin/microkit" \
      --set-default MICROKIT_SDK "$out/lib/microkit"

    runHook postInstall
  '';

  postInstall = ''
    # Target binaries do not depend on the host's Nix store.
    nuke-refs "$out"/lib/microkit/board/*/*/{elf/*.elf,lib/*.a}
  '';

  passthru = { inherit sel4Src; };

  meta = {
    description = "A simple operating system framework for the seL4 microkernel";
    homepage = "https://docs.sel4.systems/projects/microkit/";
    maintainers = [ lib.maintainers.r4v3n6101 ];
    license = with lib.licenses; [
      bsd2
      gpl2Only
      cc-by-sa-40
    ];
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
    ];
    mainProgram = "microkit";
    outputsToInstall = [ "out" ];
  };
})
