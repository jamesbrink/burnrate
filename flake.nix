{
  description = "Burnrate desktop app devshell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        linuxTauriInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
          pkgs.atk
          pkgs.cairo
          pkgs.gdk-pixbuf
          pkgs.glib
          pkgs.gtk3
          pkgs.libsoup_3
          pkgs.pango
          pkgs.webkitgtk_4_1
          pkgs.libayatana-appindicator
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.cargo-tauri
            pkgs.nodejs_22
            pkgs.openssl
            pkgs.pkg-config
            pkgs.rustup
          ] ++ linuxTauriInputs;

          shellHook = ''
            export PATH="$PWD/scripts:$PATH"
            export RUST_BACKTRACE=1
          '';
        };

        apps = {
          dev = flake-utils.lib.mkApp { drv = pkgs.writeShellScriptBin "dev" "exec ${self}/scripts/dev"; };
          build-app = flake-utils.lib.mkApp { drv = pkgs.writeShellScriptBin "build-app" "exec ${self}/scripts/build-app"; };
          check = flake-utils.lib.mkApp { drv = pkgs.writeShellScriptBin "check" "exec ${self}/scripts/check"; };
          test = flake-utils.lib.mkApp { drv = pkgs.writeShellScriptBin "test" "exec ${self}/scripts/test"; };
          fmt = flake-utils.lib.mkApp { drv = pkgs.writeShellScriptBin "fmt" "exec ${self}/scripts/fmt"; };
          clean = flake-utils.lib.mkApp { drv = pkgs.writeShellScriptBin "clean" "exec ${self}/scripts/clean"; };
        };
      });
}
