# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

## [0.2.0] - 2026-08-31

### Added

- Independent `colors` and `render` subcommands.
- TOML configuration with Noctalia and direct-path wallpaper sources.
- File-based and inline templates.
- Hex, RGB, and JSON palette output.
- Palette input through `render --colors`, including stdin with `--colors -`.
- XDG runtime cache support with a per-user `/tmp` fallback.

### Changed

- Split wallpaper discovery, palette extraction, configuration, and formatting into separate modules.
- Cache only extracted palettes so formatting remains independent and inexpensive.
- Renamed `logo_count` terminology to the consumer-neutral `color_count`.

### Removed

- All Fastfetch-specific execution, arguments, output fields, paths, and documentation.
- Legacy Quickshell wallpaper discovery.
- Cached rendered-logo files.

### Breaking changes

- Running `wallfetch` now requires the `colors` or `render` subcommand.
- Replace `wallfetch --render-logo` with `wallfetch render`.
- Configuration moved from consumer-owned paths to `$XDG_CONFIG_HOME/wallfetch/config.toml`.

[Unreleased]: https://github.com/rozeraf/wallfetch/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/rozeraf/wallfetch/compare/433b19a...v0.2.0
