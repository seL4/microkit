#
# Copyright 2024, UNSW
# SPDX-License-Identifier: BSD-2-Clause
#
{
  description = "A flake for building microkit";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/25.05";
    utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, treefmt-nix, ... }@inputs: inputs.utils.lib.eachSystem [
    "x86_64-linux"
    "aarch64-linux"
    "x86_64-darwin"
    "aarch64-darwin"
  ]
    (system:
      let
        pkgs = import nixpkgs {
          inherit system;

          overlays = [ (import rust-overlay) ];
        };

        treefmtEval = treefmt-nix.lib.evalModule pkgs (
          { ... }:
          {
            projectRootFile = "flake.nix";
            programs.nixpkgs-fmt.enable = true;
          }
        );

        pythonTool = pkgs.python312.withPackages (ps: [
          ps.mypy
          ps.black
          ps.flake8
          ps.ply
          ps.jinja2
          ps.pyaml
          ps.lxml
          ps.pyfdt
          ps.setuptools
        ]);

        rustTool = pkgs.rust-bin.fromRustupToolchainFile ./tool/microkit/rust-toolchain.toml;
      in
      {
        # for `nix fmt`
        formatter = treefmtEval.config.build.wrapper;
        # for `nix flake check`
        checks.formatting = treefmtEval.config.build.check self;

        devShells.default = pkgs.mkShell rec {
          name = "microkit-shell";

          nativeBuildInputs = with pkgs; [
            pkgsCross.x86_64-embedded.stdenv.cc.bintools.bintools
            pkgsCross.x86_64-embedded.stdenv.cc.cc
            pkgsCross.aarch64-embedded.stdenv.cc.bintools.bintools
            pkgsCross.aarch64-embedded.stdenv.cc.cc
            pkgsCross.riscv64-embedded.stdenv.cc.bintools.bintools
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
            qemu
          ];

          # Necessary for Rust bindgen
          LIBCLANG_PATH = "${pkgs.llvmPackages_18.libclang.lib}/lib";
        };
      });
}
