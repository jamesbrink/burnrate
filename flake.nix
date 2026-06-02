{
  description = "Burnrate desktop app devshell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    devshell.url = "github:numtide/devshell";
    devshell.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, devshell }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ devshell.overlays.default ];
        };
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
        mkScriptApp = name: script: description:
          (flake-utils.lib.mkApp {
            drv = pkgs.writeShellScriptBin name ''
              exec ${self}/scripts/${script} "$@"
            '';
          }) // {
            meta.description = description;
          };
        mkCommandApp = name: command: description:
          (flake-utils.lib.mkApp {
            drv = pkgs.writeShellScriptBin name command;
          }) // {
            meta.description = description;
          };
      in
      {
        devShells.default = pkgs.devshell.mkShell {
          name = "burnrate";

          motd = ''
            {202}burnrate{reset} — tray-first quota monitor ({bold}${system}{reset})
            $(type menu &>/dev/null && menu)
          '';

          packages = [
            pkgs.cargo-tauri
            pkgs.git
            pkgs.gh
            pkgs.nodejs_22
            pkgs.openssl
            pkgs.pkg-config
            pkgs.rustup
          ] ++ linuxTauriInputs;

          env = [
            {
              name = "PATH";
              prefix = "$PWD/scripts";
            }
            {
              name = "RUST_BACKTRACE";
              value = "1";
            }
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            {
              name = "PKG_CONFIG_PATH";
              value = pkgs.lib.makeSearchPath "lib/pkgconfig" (map pkgs.lib.getDev linuxTauriInputs);
            }
            {
              name = "LD_LIBRARY_PATH";
              value = pkgs.lib.makeLibraryPath linuxTauriInputs;
            }
            {
              name = "WEBKIT_DISABLE_DMABUF_RENDERER";
              value = "1";
            }
          ];

          commands = [
            {
              category = "dev";
              name = "dev";
              help = "Start Vite dev server for the dashboard UI";
              command = "exec ./scripts/dev \"$@\"";
            }
            {
              category = "build";
              name = "build-app";
              help = "Build frontend assets and the release Rust/Tauri binary";
              command = "exec ./scripts/build-app \"$@\"";
            }
            {
              category = "check";
              name = "check";
              help = "Full local gate: Rust fmt/clippy plus TypeScript typecheck";
              command = "exec ./scripts/check \"$@\"";
            }
            {
              category = "check";
              name = "test";
              help = "Run Rust unit tests and Vitest frontend tests";
              command = "exec ./scripts/test \"$@\"";
            }
            {
              category = "check";
              name = "fmt";
              help = "Format Rust and frontend files";
              command = "exec ./scripts/fmt \"$@\"";
            }
            {
              category = "build";
              name = "clean";
              help = "Remove Rust build artifacts and built frontend assets";
              command = "exec ./scripts/clean \"$@\"";
            }
            {
              category = "release";
              name = "package-crate";
              help = "Verify the crates.io package archive";
              command = "cargo package \"$@\"";
            }
          ];
        };

        apps = {
          dev = mkScriptApp "dev" "dev" "Start the Burnrate dashboard dev server";
          build-app = mkScriptApp "build-app" "build-app" "Build frontend assets and the release binary";
          check = mkScriptApp "check" "check" "Run Rust fmt/clippy and TypeScript typecheck";
          test = mkScriptApp "test" "test" "Run Rust and frontend tests";
          fmt = mkScriptApp "fmt" "fmt" "Format Rust and frontend files";
          clean = mkScriptApp "clean" "clean" "Remove build artifacts";
          package-crate = mkCommandApp "package-crate" "exec cargo package \"$@\"" "Verify the crates.io package archive";
        };
      });
}
