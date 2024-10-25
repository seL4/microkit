{
  description = "A flake for building microkit";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/24.05";
    utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay, ... }@inputs: inputs.utils.lib.eachSystem [
    "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"
  ] (system: let
    pkgs = import nixpkgs {
      inherit system;

      overlays = [ (import rust-overlay) ];
    };

    aarch64-toolchain = import nixpkgs {
      localSystem = "${system}";
      crossSystem = {
        config = "aarch64-none-elf";
      };
    };

    # pyfdt is not officially supported in Nix so we compile it ourselves
    pyfdt = with pkgs.python310Packages;
      buildPythonPackage rec {
        pname = "pyfdt";
        version = "0.3";
        src = pkgs.fetchFromGitHub {
          owner = "superna9999";
          repo = pname;
          rev = "${pname}-${version}";
          hash = "sha256-lt/Mcw3j1aTBVOVhDBSYtriDyzeJHcSli69EXLfsgDM=";
        };

        meta = with lib; {
          description = "Python Flattened Device Tree Library";
          homepage = "https://github.com/superna9999/pyfdt";
          license = with licenses; [ asl20 ];
          maintainers = with maintainers; [ wucke13 ];
        };
    };

    pythonTool = pkgs.python310.withPackages (ps: [
      ps.mypy
      ps.black
      ps.flake8
      ps.ply
      ps.jinja2
      ps.pyaml
      ps.lxml
      pyfdt
      ps.setuptools
    ]);

    microkiToolToml = nixpkgs.lib.trivial.importTOML ./tool/microkit/Cargo.toml;
    microkitToolVersion = microkiToolToml.package.rust-version;

    rustTool = pkgs.rust-bin.stable.${microkitToolVersion}.default.override {
        targets = [ pkgs.pkgsStatic.hostPlatform.rust.rustcTarget ];
    };

    in {
        devShells.default = pkgs.mkShell rec {
            name = "microkit-shell";

            nativeBuildInputs = with pkgs; [
                pkgsCross.aarch64-embedded.stdenv.cc.bintools
                pkgsCross.aarch64-embedded.stdenv.cc.cc
                pkgsCross.riscv64-embedded.stdenv.cc.bintools
                pkgsCross.riscv64-embedded.stdenv.cc.cc
                gnumake
                dtc
                expect
                pythonTool
                git
                rustTool
                pandoc
                (texlive.combine {
                    inherit (texlive) scheme-medium titlesec enumitem sfmath roboto fontaxes isodate substr tcolorbox environ pdfcol;
                })
                cmake
                ninja
                libxml2
            ];
        };
    });
}
