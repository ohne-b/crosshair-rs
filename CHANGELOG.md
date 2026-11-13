# Changelog

## [v0.1.0-alpha.4](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.4) 2026-11-13

- Move Saved and Browse beside the Crosshairs heading and remove gaps between
  their hover backgrounds.
- Give delete and unsaved-edit dialogs more space between buttons.
- Replace the custom tray styling with Windows-rendered dark menus and native
  checkmarks, submenus and keyboard navigation. Respect Windows contrast themes.
- Update Shotgun and add Small white to Browse, both with editable native layers.
  ([#11](https://github.com/ohne-b/crosshair-rs/pull/11))

## [v0.1.0-alpha.3](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.3) 2026-11-13

- Install directly through Windows with the normal administrator prompt and no
  PowerShell window. Reuse a suitable installed Microsoft runtime.
  ([#5](https://github.com/ohne-b/crosshair-rs/pull/5))
- Add optional Discord Rich Presence, enabled by default, with an app logo,
  Simple and Soldier: 76 Quote styles, and a Download button. It keeps working
  with the dashboard closed and never shares crosshair names or designs.
  ([#6](https://github.com/ohne-b/crosshair-rs/pull/6))
- Fix the tray right-click crash and add transparent taskbar icons and process
  icons. Dismiss the visibility tooltip when the pointer leaves the switch.
  ([#5](https://github.com/ohne-b/crosshair-rs/pull/5), [#7](https://github.com/ohne-b/crosshair-rs/pull/7))
- Add Saved and Browse tabs with seven built-in crosshairs that save as editable
  copies. Improve favorite stars, sorting, search and shortcut thumbnails.
  ([#8](https://github.com/ohne-b/crosshair-rs/pull/8))
- Fix odd resize dimensions rounding up, transform position jumps, drag release
  and cancellation, and Undo during a drag. Preserve freehand stroke positions
  when keyboard edits arrive before the preview finishes.
  ([#9](https://github.com/ohne-b/crosshair-rs/pull/9))
- Keep dropdowns and text fields in sync after Undo and selection changes.
  Stop an opacity drag from affecting a newly selected layer.

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
