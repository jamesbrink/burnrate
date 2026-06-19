# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.15](https://github.com/jamesbrink/burnrate/compare/v0.1.14...v0.1.15) - 2026-06-19

### Added

- add Linux desktop support ([#49](https://github.com/jamesbrink/burnrate/pull/49))

## [0.1.14](https://github.com/jamesbrink/burnrate/compare/v0.1.13...v0.1.14) - 2026-06-13

### Fixed

- *(prefs)* responsive layout, native scroll feel, standard button proportions ([#47](https://github.com/jamesbrink/burnrate/pull/47))

## [0.1.13](https://github.com/jamesbrink/burnrate/compare/v0.1.12...v0.1.13) - 2026-06-12

### Added

- *(prefs)* modal account wizard replacing the always-visible dual-purpose form ([#46](https://github.com/jamesbrink/burnrate/pull/46))
- *(tray)* click-to-expand card drill-down replacing the accounts list ([#45](https://github.com/jamesbrink/burnrate/pull/45))

### Fixed

- *(ui)* suppress WebKit context menu, selection polish, Esc dismisses tray ([#44](https://github.com/jamesbrink/burnrate/pull/44))
- *(startup)* surface fatal init errors in a native alert instead of dying silently ([#42](https://github.com/jamesbrink/burnrate/pull/42))

## [0.1.12](https://github.com/jamesbrink/burnrate/compare/v0.1.11...v0.1.12) - 2026-06-12

### Added

- claudex-powered local usage insights and GitHub Copilot provider ([#39](https://github.com/jamesbrink/burnrate/pull/39))

### Other

- *(install)* cover the GCC-as-cc compile failure for cargo install on macOS ([#41](https://github.com/jamesbrink/burnrate/pull/41))

## [0.1.11](https://github.com/jamesbrink/burnrate/compare/v0.1.10...v0.1.11) - 2026-06-11

### Added

- *(updater)* system notification for background-found updates and a manual-check result dialog ([#37](https://github.com/jamesbrink/burnrate/pull/37))

## [0.1.10](https://github.com/jamesbrink/burnrate/compare/v0.1.9...v0.1.10) - 2026-06-11

### Fixed

- *(claude)* auto-refresh OAuth tokens for managed secondary accounts ([#35](https://github.com/jamesbrink/burnrate/pull/35))

## [0.1.9](https://github.com/jamesbrink/burnrate/compare/v0.1.8...v0.1.9) - 2026-06-10

### Other

- *(install)* correct iconv-fix claims to match cargo's real config behavior ([#33](https://github.com/jamesbrink/burnrate/pull/33))

## [0.1.8](https://github.com/jamesbrink/burnrate/compare/v0.1.7...v0.1.8) - 2026-06-10

### Fixed

- *(install)* ship cargo config pinning Apple cc so cargo install works with non-Apple cc in PATH ([#32](https://github.com/jamesbrink/burnrate/pull/32))

### Other

- VitePress docs site on GitHub Pages, README cleanup, AGENTS.md sync ([#30](https://github.com/jamesbrink/burnrate/pull/30))

## [0.1.7](https://github.com/jamesbrink/burnrate/compare/v0.1.6...v0.1.7) - 2026-06-09

### Fixed

- *(auth)* isolate secondary sign-ins, harden credential teardown, and strip inherited credential env vars ([#28](https://github.com/jamesbrink/burnrate/pull/28))

## [0.1.6](https://github.com/jamesbrink/burnrate/compare/v0.1.5...v0.1.6) - 2026-06-05

### Added

- *(storage)* migrate config to sqlite
- *(tray)* adapt popover height to screen space

### Fixed

- *(config)* preserve valid config on schema errors

## [0.1.5](https://github.com/jamesbrink/burnrate/compare/v0.1.4...v0.1.5) - 2026-06-03

### Added

- *(aws)* add Cost Explorer usage provider

## [0.1.4](https://github.com/jamesbrink/burnrate/compare/v0.1.3...v0.1.4) - 2026-06-03

### Fixed

- *(login)* repair Claude Code sign-in flow ([#18](https://github.com/jamesbrink/burnrate/pull/18))

## [0.1.3](https://github.com/jamesbrink/burnrate/compare/v0.1.2...v0.1.3) - 2026-06-03

### Added

- in-app auto-updates (stable/nightly) + tray settings gear ([#16](https://github.com/jamesbrink/burnrate/pull/16))

## [0.1.2](https://github.com/jamesbrink/burnrate/compare/v0.1.1...v0.1.2) - 2026-06-03

### Added

- multi-account login for Claude Code & Codex (email, drag-reorder) ([#13](https://github.com/jamesbrink/burnrate/pull/13))

## [0.1.1](https://github.com/jamesbrink/burnrate/compare/v0.1.0...v0.1.1) - 2026-06-03

### Added

- *(provider)* add Runpod monitoring
- native macOS tray popover + Preferences light/dark, multi-monitor & dev fixes ([#10](https://github.com/jamesbrink/burnrate/pull/10))
