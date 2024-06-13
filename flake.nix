{
  description = "A flake for building microkit";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/24.05";
    utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay, ... }@inputs: inputs.utils.lib.eachSystem [
    # Add the system/architecture you would like to support here. Note that not
    # all packages in the official nixpkgs support all platforms.
    "x86_64-linux" "i686-linux" "aarch64-linux" "x86_64-darwin"
  ] (system: let
    pkgs = import nixpkgs {
      inherit system;

      # Add overlays here if you need to override the nixpkgs
      # official packages.
      overlays = [ (import rust-overlay) ];
      
      # Uncomment this if you need unfree software (e.g. cuda) for
      # your project.
      #
      /* config.allowUnfree = true;
      config.allowUnsupportedsystem = true; */
    };

    aarch64-toolchain = import nixpkgs {
      localSystem = "${system}";
      crossSystem = {
        config = "aarch64-none-elf";
      };
    };

    libfdt = with pkgs.python310Packages;
      buildPythonPackage rec {
        pname = "pylibfdt";
        version = "1.6.1";
        src = fetchPypi {
          inherit pname version;
          hash = "sha256-kMZnxa30TGqy8TvcVmWYiXeEx7eBvtkQZOc3O9Jwt3g=";
        };
        propagatedBuildInputs = [
          pip
          setuptools_scm
        ];
        nativeBuildInputs = [
          setuptools_scm
          pkgs.swig
        ];
      };


    pyfdt = with pkgs.python310Packages;
      buildPythonPackage rec {
        pname = "pyfdt";
        version = "0.3";
        src = fetchPypi {
          inherit pname version;
          hash = "sha256-YWAcIAX/OUolpshMbaIIi7+IgygDhADSfk7rGwS59PA=";
        };
        propagatedBuildInputs = [
          pip
          setuptools_scm
        ];
        nativeBuildInputs = [
          setuptools_scm
          pkgs.swig
        ];
      };

    pythonTool = pkgs.python310.withPackages (ps: [
      ps.mypy
      ps.black
      ps.flake8
      ps.ply
      ps.jinja2
      ps.pyaml
      libfdt
      pyfdt
      ps.setuptools
    ]);

  
    rust_version = "latest";
    rustTool = pkgs.rust-bin.nightly.${rust_version}.default.override {
        targets = [ "x86_64-unknown-linux-musl" ];
    };



  in {
    devShells.default = pkgs.mkShell rec {
      # Update the name to something that suites your project.
      name = "microkit-shell";

      packages = with pkgs; [
        gnumake
        dtc
        expect
        pythonTool
        git
        aarch64-toolchain.buildPackages.gcc
        gcc
        rustTool
        pandoc
        (texlive.combine {
          inherit (texlive) scheme-medium titlesec;
        })
        cmake
        ninja
        libxml2
        libxcrypt
      ];
    };

    /* packages.default = pkgs.stdenv.mkDerivation {
      buildInputs = with pkgs; [
        gnumake
        dtc
        expect
        python311
        git
        aarch64-toolchain.buildPackages.gcc
        gcc
        rustTool
        pandoc
        (texlive.combine {
          inherit (texlive) scheme-medium titlesec;
        })
        cmake
        ninja
        libxml2
        libxcrypt
        pythonPkgs.mypy
        pythonPkgs.black
        pythonPkgs.flake8
        pythonPkgs.ply
        pythonPkgs.jinja2
        pythonPkgs.pyaml
        pythonPkgs.libfdt
      ];
      buildPhase = ''
        python -m venv ./pyenv
        ./pyenv/bin/pip install -r requirements.txt
        ./pyenv/bin/python build_sdk.py --sel4=../seL4
      '';
    }; */
  });
}

