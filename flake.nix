{
  description = "Desktop usage monitor for Claude Code, Codex, GitHub Copilot, OpenRouter, Runpod, and AWS quotas, credits, spend, and subscription limits.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

    devshell.url = "github:numtide/devshell";
    devshell.inputs.nixpkgs.follows = "nixpkgs";

    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs:
    let
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      cargoPackage = cargoToml.package;
    in
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devshell.flakeModule
        inputs.treefmt-nix.flakeModule
      ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      flake.overlays.default =
        final: _prev:
        let
          system = final.stdenv.hostPlatform.system;
        in
        if builtins.hasAttr system inputs.self.packages then
          {
            burnrate = inputs.self.packages.${system}.default;
          }
        else
          { };

      perSystem =
        {
          system,
          lib,
          config,
          ...
        }:
        let
          pkgs = import inputs.nixpkgs {
            localSystem = system;
          };

          license =
            if cargoPackage.license == "MIT" && builtins.pathExists ./LICENSE then
              lib.licenses.mit
            else
              throw "Burnrate flake metadata expects Cargo.toml license = MIT and a root LICENSE file";

          source = lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              let
                name = baseNameOf path;
                rel = lib.removePrefix "${toString ./.}/" (toString path);
              in
              !(
                (
                  type == "directory"
                  && builtins.elem rel [
                    ".direnv"
                    ".git"
                    "coverage"
                    "dist"
                    "node_modules"
                    "target"
                  ]
                )
                || name == "result"
                || lib.hasPrefix "result-" name
              );
          };

          linuxTauriInputs = lib.optionals pkgs.stdenv.isLinux [
            pkgs.webkitgtk_4_1
            pkgs.gtk3
            pkgs.cairo
            pkgs.pango
            pkgs.harfbuzz
            pkgs.atk
            pkgs.gdk-pixbuf
            pkgs.libsoup_3
            pkgs.glib
            pkgs.glib-networking
            pkgs.dbus
            pkgs.zlib
            pkgs.openssl
            pkgs.libayatana-appindicator
            pkgs.gsettings-desktop-schemas
          ];

          linuxGSettingsSchemaDirs = lib.optionals pkgs.stdenv.isLinux [
            "${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}"
            "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}"
          ];

          linuxPkgConfigPath = lib.concatStringsSep ":" [
            (lib.makeSearchPath "lib/pkgconfig" (map lib.getDev linuxTauriInputs))
            (lib.makeSearchPath "share/pkgconfig" (map lib.getDev linuxTauriInputs))
          ];

          linuxCargoTarget =
            {
              x86_64-linux = "X86_64_UNKNOWN_LINUX_GNU";
              aarch64-linux = "AARCH64_UNKNOWN_LINUX_GNU";
            }
            .${system} or null;

          darwinTauriInputs = [ ];

          mkScriptApp = name: script: description: {
            type = "app";
            program = "${pkgs.writeShellScriptBin name ''
              exec ${inputs.self}/scripts/${script} "$@"
            ''}/bin/${name}";
            meta.description = description;
          };

          mkCommandApp = name: command: description: {
            type = "app";
            program = "${pkgs.writeShellScriptBin name command}/bin/${name}";
            meta.description = description;
          };
        in
        {
          _module.args.pkgs = pkgs;

          packages = rec {
            burnrate = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
              pname = cargoPackage.name;
              inherit (cargoPackage) version;

              src = source;

              cargoRoot = ".";
              buildAndTestSubdir = finalAttrs.cargoRoot;

              cargoLock.lockFile = ./Cargo.lock;

              # The reproducible Nix build has no Tauri signing key, but
              # `createUpdaterArtifacts: true` makes `tauri build` try to sign
              # the updater bundle and fail ("public key found, but no private
              # key"). Updater artifacts are a release-only concern (produced by
              # the signed GitHub release/nightly workflows), so disable them
              # here.
              postPatch = ''
                substituteInPlace tauri.conf.json \
                  --replace-fail '"createUpdaterArtifacts": true' '"createUpdaterArtifacts": false'
              '';

              # .cargo/config.toml pins /usr/bin/cc as the darwin linker so
              # `cargo install` survives a non-Apple cc on PATH; inside the
              # Nix sandbox that path is unavailable, so point cargo back at
              # the stdenv toolchain (env vars take precedence over the file).
              env = lib.optionalAttrs pkgs.stdenv.isDarwin {
                CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER = "${pkgs.stdenv.cc}/bin/cc";
                CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER = "${pkgs.stdenv.cc}/bin/cc";
              };

              npmRoot = ".";
              npmDeps = pkgs.fetchNpmDeps {
                name = "${finalAttrs.pname}-${finalAttrs.version}-npm-deps";
                inherit (finalAttrs) src;
                hash = "sha256-6MjiV5pzJ1KGe67bZmja5vPjK91H+7g7G1pq7A4ahSs=";
              };

              nativeBuildInputs = [
                pkgs.cargo-tauri.hook
                pkgs.nodejs_22
                pkgs.npmHooks.npmConfigHook
              ]
              ++ lib.optionals pkgs.stdenv.isLinux [
                pkgs.pkg-config
                pkgs.wrapGAppsHook3
              ]
              ++ lib.optionals pkgs.stdenv.isDarwin [
                pkgs.makeBinaryWrapper
              ];

              buildInputs = linuxTauriInputs ++ darwinTauriInputs;

              tauriBuildFlags = lib.optionals pkgs.stdenv.isDarwin [
                "--no-sign"
              ];

              postInstall = lib.optionalString pkgs.stdenv.isDarwin ''
                mkdir -p "$out/bin"
                makeWrapper "$out/Applications/Burnrate.app/Contents/MacOS/burnrate" "$out/bin/burnrate"
              '';

              meta = {
                inherit (cargoPackage) description homepage;
                changelog = "${cargoPackage.repository}/releases";
                inherit license;
                mainProgram = cargoPackage.name;
                platforms = lib.platforms.linux ++ lib.platforms.darwin;
              };
            });

            default = burnrate;
          };

          apps = {
            burnrate = {
              type = "app";
              program = lib.getExe config.packages.burnrate;
              meta.description = cargoPackage.description;
            };
            default = config.apps.burnrate;
            dev = mkScriptApp "dev" "dev" "Start the Burnrate Tauri desktop app and tray icon";
            build-app = mkScriptApp "build-app" "build-app" "Build frontend assets and the release binary";
            check = mkScriptApp "check" "check" "Run Rust fmt/clippy and TypeScript typecheck";
            test = mkScriptApp "test" "test" "Run Rust and frontend tests";
            fmt = mkScriptApp "fmt" "fmt" "Format Rust and frontend files";
            clean = mkScriptApp "clean" "clean" "Remove build artifacts";
            package-dmg = mkScriptApp "package-dmg" "package-dmg" "Package the macOS .dmg bundle";
            package-crate =
              mkCommandApp "package-crate" "exec cargo package \"$@\""
                "Verify the crates.io package archive";
          };

          devshells.default = {
            name = cargoPackage.name;

            motd = ''
              {202}${cargoPackage.name}{reset} — desktop usage monitor ({bold}${system}{reset})
              $(type menu &>/dev/null && menu)
            '';

            packages = [
              pkgs.cargo
              pkgs.cargo-tauri
              pkgs.clippy
              pkgs.git
              pkgs.gh
              pkgs.nodejs_22
              pkgs.openssl
              pkgs.pkg-config
              pkgs.rustc
              pkgs.rustfmt
              pkgs.stdenv.cc
              config.treefmt.build.wrapper
            ]
            ++ linuxTauriInputs
            ++ darwinTauriInputs;

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
            ++ lib.optionals pkgs.stdenv.isDarwin [
              {
                name = "CC";
                value = "/usr/bin/cc";
              }
              {
                name = "CXX";
                value = "/usr/bin/c++";
              }
              {
                name = "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER";
                value = "/usr/bin/cc";
              }
              {
                name = "CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER";
                value = "/usr/bin/cc";
              }
              # The dev codesign runner lives here (not in .cargo/config.toml)
              # because that file ships in the crates.io package and the
              # runner script deliberately does not.
              {
                name = "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER";
                eval = "$PRJ_ROOT/scripts/dev-codesign-run.sh";
              }
              {
                name = "CARGO_TARGET_X86_64_APPLE_DARWIN_RUNNER";
                eval = "$PRJ_ROOT/scripts/dev-codesign-run.sh";
              }
            ]
            ++ lib.optionals pkgs.stdenv.isLinux [
              {
                name = "CC";
                value = "${pkgs.stdenv.cc}/bin/cc";
              }
              {
                name = "CXX";
                value = "${pkgs.stdenv.cc}/bin/c++";
              }
              {
                name = "CARGO_TARGET_${linuxCargoTarget}_LINKER";
                value = "${pkgs.stdenv.cc}/bin/cc";
              }
              {
                name = "PKG_CONFIG_PATH";
                value = linuxPkgConfigPath;
              }
              {
                name = "LD_LIBRARY_PATH";
                value = lib.makeLibraryPath linuxTauriInputs;
              }
              {
                name = "GIO_EXTRA_MODULES";
                prefix = "${pkgs.glib-networking}/lib/gio/modules";
              }
              {
                name = "XDG_DATA_DIRS";
                prefix = lib.concatStringsSep ":" (
                  (map (pkg: "${pkg}/share") linuxTauriInputs) ++ linuxGSettingsSchemaDirs
                );
              }
            ];

            commands = [
              {
                category = "dev";
                name = "dev";
                help = "Start the Tauri desktop app and tray icon";
                command = "exec ./scripts/dev \"$@\"";
              }
              {
                category = "build";
                name = "build-app";
                help = "Build frontend assets and the release Rust/Tauri binary";
                command = "exec ./scripts/build-app \"$@\"";
              }
              {
                category = "build";
                name = "build-pure";
                help = "Build Burnrate through the pure Nix package target";
                command = "exec nix build .#burnrate \"$@\"";
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
                help = "Format Rust, Nix, TOML, JSON, Markdown, and CSS";
                command = "exec treefmt \"$@\"";
              }
              {
                category = "build";
                name = "clean";
                help = "Remove Rust build artifacts and built frontend assets";
                command = "exec ./scripts/clean \"$@\"";
              }
              {
                category = "release";
                name = "package-dmg";
                help = "Package the macOS .dmg bundle (and the .app it wraps)";
                command = "exec ./scripts/package-dmg \"$@\"";
              }
              {
                category = "release";
                name = "package-crate";
                help = "Verify the crates.io package archive";
                command = "cargo package \"$@\"";
              }
              {
                category = "docs";
                name = "docs-dev";
                help = "Start the VitePress dev server for the docs site (hot reload)";
                command = "cd $PRJ_ROOT/website && npm install && npm run dev \"$@\"";
              }
              {
                category = "docs";
                name = "docs-build";
                help = "Build the docs site (static output in website/.vitepress/dist)";
                command = "cd $PRJ_ROOT/website && npm ci && npm run build";
              }
            ];
          };

          treefmt = {
            projectRootFile = "flake.nix";
            programs.nixfmt.enable = true;
            programs.rustfmt.enable = true;
            programs.prettier.enable = true;
            programs.taplo.enable = true;
            settings.formatter.prettier.excludes = [
              "*.cjs"
              "*.js"
              "*.jsx"
              "*.mjs"
              "*.mts"
              "*.ts"
              "*.tsx"
              "gen/**"
              "package-lock.json"
              "website/package-lock.json"
              "node_modules/**"
              "dist/**"
              "coverage/**"
              "target/**"
              # Generated by release-plz; not prettier-formatted, so excluding it
              # keeps `nix flake check`'s treefmt gate green on release PRs.
              "CHANGELOG.md"
            ];
            settings.formatter.taplo.excludes = [
              "target/**"
              "node_modules/**"
            ];
          };
        };
    };
}
