# Installation

Requires Windows 11, x64. Xbox Game Bar is needed only for Game Bar display mode.

## Install a release

1. Open [Releases](https://github.com/ohne-b/crosshair-rs/releases) and choose a version.
2. Download `crosshair-rs-VERSION-windows-x64-setup.exe`.
3. Run the setup EXE. It includes the application, Game Bar widget and required
   Microsoft runtime; no other release files need downloading.
4. Approve the Windows administrator prompt using the same Windows account,
   then open **crosshair-rs** from Start.

The installer checks its embedded package and dependency before requesting approval.
Download it from this repository's Releases page: the setup EXE and Windows package
are not Windows-signed. In-app updates authenticate both using the project's release key.
No certificate import or Developer Mode is needed. Normal use does not require
administrator rights. `latest.json` and `SHA256SUMS` are update and verification metadata.

For a source build, see [Contributing](CONTRIBUTING.md#development).

## Fullscreen with Game Bar

1. Choose **Game Bar** in **Settings > Display > Display mode**.
2. Press **Win+G**, open **crosshair-rs** from Widgets, then pin it and enable click-through.
3. Position the widget on your target display, then close the Game Bar panel.

Desktop mode covers normal windows and borderless games. Fullscreen support
depends on Game Bar and the game; Vulkan/OpenGL fullscreen can be restricted.
Games and anti-cheat systems may block overlays. crosshair-rs does not inject
into games or bypass those restrictions.

## Updates

Open **Settings > Updates**, check for updates, then choose **Download**.
After verification, choose **Install and restart** and approve the Windows prompt.
The overlay and editor close during installation. Your settings are retained.

Checks use the latest stable release. For alpha-to-alpha updates, use the newer
release's installer. Automatic checks can be disabled in Settings; downloads and
installation always require your action.

Alpha 1 used the earlier multi-file installer. Install a newer alpha with its setup
EXE before using the new update format; your settings are retained.

Before uninstalling, [export your data](data-and-privacy.md). Windows removes the
installed app's local data on uninstall.
