# Changelog

## [v0.4.0-rc.3](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.4.0-rc.3) 2026-11-23

- feat: Add square direction arrows to the darker Movement header and a divider between the keyboard and controller rows. ([#51](https://github.com/ohne-b/crosshair-rs/pull/51))
- feat: Show empty keybinds with a muted **Unbound** label. All four Movement labels fit at 200% text size. ([#51](https://github.com/ohne-b/crosshair-rs/pull/51))

## [v0.4.0-rc.2](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.4.0-rc.2) 2026-11-22

- feat: Edit keyboard and controller Movement bindings in a shared direction grid. Click any empty cell to bind it. ([#49](https://github.com/ohne-b/crosshair-rs/pull/49))
- fix: Show a Movement binding’s clear button beside its label on hover or keyboard focus, with matching highlight heights. ([#49](https://github.com/ohne-b/crosshair-rs/pull/49))
- fix: Hide the star and trash buttons when the pointer leaves a saved crosshair after favoriting or unfavoriting it. Keyboard focus still reveals both buttons. ([#49](https://github.com/ohne-b/crosshair-rs/pull/49))
- fix: Add space between entries in the profile picker and the library sort menu. ([#49](https://github.com/ohne-b/crosshair-rs/pull/49))
- fix: Center the Designer layer visibility and delete highlights while keeping the full row height clickable. ([#49](https://github.com/ohne-b/crosshair-rs/pull/49))

## [v0.4.0-rc.1](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.4.0-rc.1) 2026-11-22

- Bind a controller button, trigger or stick direction alongside a keyboard or mouse shortcut for any action or saved crosshair. ([#47](https://github.com/ohne-b/crosshair-rs/pull/47))
- Edit both bindings side by side in Keybinds, with a separate Movement row for each device. Clearing a binding keeps the saved crosshair shortcut available to rebind. ([#47](https://github.com/ohne-b/crosshair-rs/pull/47))

## [v0.3.0](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.3.0) 2026-11-22

- fix: Drag a closed crosshair gap open again in whole pixels. ([#43](https://github.com/ohne-b/crosshair-rs/pull/43))
- fix: Click through gaps in a layer to select the shape underneath. ([#43](https://github.com/ohne-b/crosshair-rs/pull/43))
- feat: Drop a preset onto the library import panel or browse for a file. ([#43](https://github.com/ohne-b/crosshair-rs/pull/43))
- feat: Drop images onto the Designer’s Image panel. Both import icons light up when you drag a supported file over them. ([#43](https://github.com/ohne-b/crosshair-rs/pull/43))
- fix: Skip wheel scrolling animations when Windows animation effects are off. ([#42](https://github.com/ohne-b/crosshair-rs/pull/42))
- fix: Use the Windows Start entry and retire the installer’s `/STARTMENU` switch. ([#41](https://github.com/ohne-b/crosshair-rs/pull/41))
- fix: Compare stable release notes with the previous stable tag. ([#41](https://github.com/ohne-b/crosshair-rs/pull/41))
- docs: Download the Windows installer directly from the README. ([#38](https://github.com/ohne-b/crosshair-rs/pull/38))
- feat: See how Crosshair V2 and OpenCrosshair compare in the website and README RAM charts. ([#44](https://github.com/ohne-b/crosshair-rs/pull/44))
- fix: Keep website sections below the fixed header and scroll back from Credits to the RAM chart. ([#44](https://github.com/ohne-b/crosshair-rs/pull/44))

## [v0.3.0-rc.1](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.3.0-rc.1) 2026-11-22

- fix: Reopen a closed crosshair gap in whole pixels when dragging a resize handle. ([#43](https://github.com/ohne-b/crosshair-rs/pull/43))
- fix: Click through gaps in a layer to select the shape underneath. ([#43](https://github.com/ohne-b/crosshair-rs/pull/43))
- feat: Drop preset files onto the library import panel or browse for a file. ([#43](https://github.com/ohne-b/crosshair-rs/pull/43))
- feat: Drop images onto the Designer’s Image panel. Both panels highlight their icon when you drag a supported file over them. ([#43](https://github.com/ohne-b/crosshair-rs/pull/43))
- fix: Skip wheel scrolling animations when Windows animation effects are off. ([#42](https://github.com/ohne-b/crosshair-rs/pull/42))
- fix: Use the Windows Start entry and retire the installer’s legacy `/STARTMENU` switch. ([#41](https://github.com/ohne-b/crosshair-rs/pull/41))
- fix: Compare stable release notes with the previous stable tag. ([#41](https://github.com/ohne-b/crosshair-rs/pull/41))
- docs: Download the stable Windows installer directly from the README. ([#38](https://github.com/ohne-b/crosshair-rs/pull/38))

## [v0.2.0](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.2.0) 2026-11-21

- feat: Resize shapes, gaps and dots with corner handles. Rotated handles show the matching cursor, and sliders give finer control over small values. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- fix: Keep small crosshairs symmetric when zoomed, including touching tips, arcs and blur. The center marker stays visible against the pixel grid and alignment guides appear while dragging. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- refactor: Zoom the canvas around the mouse pointer with the wheel. The Fit and 100% buttons have been removed. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- feat: Export an editable JSON preset directly from the current draft, including unsaved changes. The File menu has labeled actions with icons, and Save highlights pending edits. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- feat: See available updates above Settings in the editor sidebar. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- fix: Show a checkmark after saving a crosshair from Browse and allow saving another copy. ([#31](https://github.com/ohne-b/crosshair-rs/pull/31))
- fix: Restore rendering in the pinned Game Bar widget. Display settings show when to open and pin the widget and keep the desktop crosshair visible until Game Bar is ready. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Preserve shortcut choices during upgrades and let setup retry if Windows reports the app is in use. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Keep Preview and release sessions separate and identify their widgets in the Game Bar picker. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Give the engine, editor and Game Bar widget distinct process names and crosshair icons in Task Manager. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Use the native Windows Start entry and show the crosshair icon on new desktop shortcuts, the taskbar and the app switcher. ([#34](https://github.com/ohne-b/crosshair-rs/pull/34))
- fix: Name the Task Manager group `crosshair-rs` and keep the individual process names visible underneath. ([#34](https://github.com/ohne-b/crosshair-rs/pull/34))
- fix: Apply the launch-at-sign-in setting on existing installations when opening the app from Start or Game Bar. ([#29](https://github.com/ohne-b/crosshair-rs/pull/29))
- fix: Read the sign-in preference when General settings opens so starting the app or switching displays no longer wakes Runtime Broker for that setting. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Let Windows close Runtime Broker after reading or changing the sign-in setting while the overlay keeps running. ([#34](https://github.com/ohne-b/crosshair-rs/pull/34))
- feat: Link to the community Discord from the website and README. ([#27](https://github.com/ohne-b/crosshair-rs/pull/27))
- docs: Clarify Windows availability and memory use on the homepage. ([#28](https://github.com/ohne-b/crosshair-rs/pull/28))

## [v0.2.0-rc.2](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.2.0-rc.2) 2026-11-21

- fix: Give new desktop shortcuts the crosshair icon. Setup now uses the Start menu entry that Windows creates automatically. ([#34](https://github.com/ohne-b/crosshair-rs/pull/34))
- fix: Show the crosshair icon on the Windows taskbar and in the app switcher. ([#34](https://github.com/ohne-b/crosshair-rs/pull/34))
- fix: Name the overlay’s Task Manager group `crosshair-rs` while keeping separate names for the engine and editor processes. ([#34](https://github.com/ohne-b/crosshair-rs/pull/34))
- fix: Let Windows close Runtime Broker after reading or changing the launch-at-sign-in setting, even while the overlay keeps running. ([#34](https://github.com/ohne-b/crosshair-rs/pull/34))

## [v0.2.0-rc.1](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.2.0-rc.1) 2026-11-21

- feat: Drag corner handles to resize shapes, crosshair gaps and dots. Sliders give finer control over small values, and the resize cursors follow rotated handles. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- fix: Keep small crosshairs symmetric and preserve touching tips, arcs and blurred edges when zoomed. The canvas center is easier to distinguish from the pixel grid, and alignment guides appear only while moving layers. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- refactor: Remove the canvas Fit and 100% buttons. The mouse wheel zooms around the pointer. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- feat: Export the current draft as an editable JSON preset, including unsaved layer and animation changes. The File menu adds icons and fits its contents, and Save highlights pending edits. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- feat: Show available updates above Settings in the editor sidebar. ([#32](https://github.com/ohne-b/crosshair-rs/pull/32))
- fix: Restore rendering in the pinned Game Bar widget. Display settings guide you through opening and pinning, keep Desktop active until the widget is ready, and switch back after the widget clears. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Preserve shortcut choices and customizations during upgrades and let setup retry when Windows reports the app is in use. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Keep Preview and release sessions separate when both are installed. The Game Bar widget picker identifies each installation. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Use the crosshair icon and distinct editor, engine and Game Bar process names in Task Manager. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Read the Windows launch-at-sign-in setting when General settings opens. Starting the app and switching displays no longer wake Runtime Broker for this setting. ([#30](https://github.com/ohne-b/crosshair-rs/pull/30))
- fix: Apply the launch-at-sign-in option on existing installations when the app starts from the Start menu or Game Bar. ([#29](https://github.com/ohne-b/crosshair-rs/pull/29))
- fix: Show a checkmark after saving a Browse crosshair and allow saving another copy. ([#31](https://github.com/ohne-b/crosshair-rs/pull/31))
- feat: Add the community Discord link to the website navigation and README. ([#27](https://github.com/ohne-b/crosshair-rs/pull/27))
- docs: Update the homepage introduction to describe availability and memory use. ([#28](https://github.com/ohne-b/crosshair-rs/pull/28))

## [v0.1.0](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0) 2026-11-20

- First stable release for Windows 11 x64. Use a desktop crosshair overlay or the bundled Xbox Game Bar widget. Closing the editor keeps the overlay running.
  ([#2](https://github.com/ohne-b/crosshair-rs/pull/2))
- Create layered crosshairs, import images, undo edits and export PNG or SVG. Equip a built-in design from Browse or save a copy to edit. Small, whole-pixel strokes stay solid when you resize a design or change the overlay scale.
  ([#9](https://github.com/ohne-b/crosshair-rs/pull/9), [#13](https://github.com/ohne-b/crosshair-rs/pull/13), [#15](https://github.com/ohne-b/crosshair-rs/pull/15))
- Set per-profile Aim actions and animate layers independently on Aim, Fire and Movement. Pan the canvas during animation playback, configure all four movement keys in one row, and bind your map key to hide the crosshair while held.
  ([#17](https://github.com/ohne-b/crosshair-rs/pull/17), [#20](https://github.com/ohne-b/crosshair-rs/pull/20), [#24](https://github.com/ohne-b/crosshair-rs/pull/24))
- Install from one Windows setup file and choose shortcuts, launch at sign-in and whether to open the app after setup. Settings shows the Windows sign-in preference and download progress. In-app updates verify the installer and its embedded package, then reopen the app after installation. Prerelease updates are optional.
  ([#4](https://github.com/ohne-b/crosshair-rs/pull/4), [#14](https://github.com/ohne-b/crosshair-rs/pull/14), [#20](https://github.com/ohne-b/crosshair-rs/pull/20))
- Join the community from the editor sidebar or optional Discord Rich Presence. The Movement row now matches its neighbors, and button tooltips have clean rounded corners.
  ([#6](https://github.com/ohne-b/crosshair-rs/pull/6), [#23](https://github.com/ohne-b/crosshair-rs/pull/23), [#25](https://github.com/ohne-b/crosshair-rs/pull/25))

## [v0.1.0-alpha.7](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.7) 2026-11-16

- Add profile-specific Aim actions for visibility and saved-crosshair switching,
  with Fire and Reload bindings. Keep the Global toggle tied to the master switch.
- Add simple per-layer animations in Designer's top-right Animate inspector,
  local preview and overlay playback with the dashboard closed. Preserve effects
  in saved documents and backups; existing designs stay static. Set duration to
  0 ms for instant changes.
- Equip Browse crosshairs by clicking their cards without adding them to Saved.
  The bookmark button still saves an editable copy.
- Fix persistent mouse focus highlights, align Aim controls and center their square
  previews. Restore compact notification strips that fill the panel corners.
  ([#17](https://github.com/ohne-b/crosshair-rs/pull/17))

## [v0.1.0-alpha.6](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.6) 2026-11-14

- Add an optional Include prereleases switch for alpha, beta and release-candidate
  updates. Keep complete version names and the existing signed setup verification.
- Handle update checks cleanly when no stable release has been published.
- Fix the layer visibility button and color pickers missing the final drag position.
- Round the Saved/Browse buttons and warning banners, align warning controls, and
  remove the extra Discord settings description.
- Look up launch-at-sign-in tasks across the package and clarify missing-installation errors.
  ([#14](https://github.com/ohne-b/crosshair-rs/pull/14))

## [v0.1.0-alpha.5](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.5) 2026-11-14

- Keep small whole-pixel crosshair strokes solid when resizing or changing overlay
  scale. Preserve smooth fractional and rotated edges.
  ([#13](https://github.com/ohne-b/crosshair-rs/pull/13))
- Keep header crosshair previews centered across sizes and display scaling.
- Add TeloqR (CS Pro) to Browse as an editable crosshair.
- Reduce the Slint attribution badge in the README.
  ([#12](https://github.com/ohne-b/crosshair-rs/pull/12))

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
