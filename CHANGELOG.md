# Changelog

## [v0.1.0-alpha.2](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.2) 2026-11-13

- Install from one Windows setup EXE with the Game Bar package and Microsoft
  runtime included. Separate installer scripts, verifier tools and signatures
  are no longer release downloads.
- Keep update signatures in the update metadata and verify both the installer
  and its embedded application package. Upgrade alpha 1 with the new setup EXE.
  ([#4](https://github.com/ohne-b/crosshair-rs/pull/4))

## [v0.1.0-alpha.1](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.1) 2026-11-12

- First Windows 11 x64 alpha, with a desktop crosshair overlay and bundled Xbox
  Game Bar widget. ([#2](https://github.com/ohne-b/crosshair-rs/pull/2))
- Create layered crosshairs with live previews, undo, image import and PNG/SVG
  export. Organize presets and profiles, and assign global keyboard or mouse shortcuts.
- Install with the included `Install.ps1`. The Windows package is unsigned;
  the installer verifies its separate Minisign signature before requesting administrator approval.
- Fullscreen support depends on Game Bar and the game. Alpha-to-alpha upgrades
  use the release installer; the in-app updater checks stable releases only.
