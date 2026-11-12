# Changelog

## [v0.1.0-alpha.1](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.1) 2026-11-12

- First Windows 11 x64 alpha, with a desktop crosshair overlay and bundled Xbox
  Game Bar widget. ([#2](https://github.com/ohne-b/crosshair-rs/pull/2))
- Create layered crosshairs with live previews, undo, image import and PNG/SVG
  export. Organize presets and profiles, and assign global keyboard or mouse shortcuts.
- Install with the included `Install.ps1`. The Windows package is unsigned;
  the installer verifies its separate Minisign signature before requesting administrator approval.
- Fullscreen support depends on Game Bar and the game. Alpha-to-alpha upgrades
  use the release installer; the in-app updater checks stable releases only.
