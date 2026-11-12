# Installation

Requires Windows 11, x64. Xbox Game Bar is needed only for Game Bar display mode.

## Install a release

1. Open [Releases](https://github.com/ohne-b/crosshair-rs/releases) and choose a version.
2. Download its `.msix`, `.minisig`, `Install.ps1`, `minisign.exe` and
   `Microsoft.VCLibs.x64.14.00.appx` into one folder.
3. Open PowerShell in that folder and run:

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File .\Install.ps1
   ```

4. Approve the Windows administrator prompt using the same Windows account,
   then open **crosshair-rs** from Start.

The installer verifies the download before requesting approval. The Windows
package is unsigned; its separate updater signature authenticates the download.
No certificate import or Developer Mode is needed. Normal use does not require
administrator rights. Add `-VerifyOnly` to check the files without installing.

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

Before uninstalling, [export your data](data-and-privacy.md). Windows removes the
installed app's local data on uninstall.
