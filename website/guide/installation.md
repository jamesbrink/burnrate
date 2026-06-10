# Installation

## Native bundles

Download the bundle for your platform from
[GitHub Releases](https://github.com/jamesbrink/burnrate/releases/latest):

- **macOS** — `.dmg` (universal). Release builds are Developer ID-signed and
  notarized.
- **Linux** — `.AppImage`, `.deb`, and `.rpm`.
- **Windows** — `.msi` installer.

## From crates.io

The crate ships the prebuilt UI, so a plain install works without Node:

```sh
cargo install burnrate
```

## With Nix

The repository is a flake exposing the package for `x86_64-linux`,
`aarch64-linux`, and `aarch64-darwin`:

```sh
nix run github:jamesbrink/burnrate
```

## Automatic updates (macOS)

Native macOS installs update themselves: a dismissible banner and the tray's
**Check for Updates…** entry offer a signature-verified one-click **Install &
Restart**.

Two channels are available under **Preferences → Updates**:

- **Stable** — tagged releases.
- **Nightly** — a rolling pre-release built from `main` after CI passes.

::: tip Keychain and code signatures
On macOS the keychain binds its "Always Allow" grant to the app's code
signature, so a signed install (the released `.dmg`) remembers the grant. An
unsigned local build re-prompts on every launch — see
[Troubleshooting](/guide/troubleshooting#keychain-re-prompts-on-every-launch).
:::
