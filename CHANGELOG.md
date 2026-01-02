# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [1.0.3] - 2026-01-03
### Fixed
- Startup now falls back to default config if the user config is invalid (instead of exiting).
- Fixed `example_config.toml` (removed duplicate `[open_with]` table).

## [1.0.4] - 2026-01-03
### Fixed
- Release/build: synchronize `Cargo.lock` with package version to keep `--locked` builds working.

## [1.0.2] - 2026-01-03
### Fixed
- Arch package/release builds: avoid Oniguruma (`onig`) linker failures by using syntect's pure-Rust regex backend.

## [1.0.1] - 2026-01-02
### Fixed
- Release pipeline fixes (Arch packaging + broader release artifacts).

## [1.0.0] - 2026-01-02
### Added
- Initial public release.
- Dual-pane TUI file manager with preview, search, markers, and configurable keybindings.
