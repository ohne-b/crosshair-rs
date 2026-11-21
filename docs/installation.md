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
It installs through Windows directly, without launching PowerShell. A suitable
installed Microsoft runtime is reused. If installation fails, setup displays the
Windows error; it does not forcibly close other applications.
Download it from this repository's Releases page: the setup EXE and Windows package
are not Windows-signed. In-app updates authenticate both using the project's release key.
No certificate import or Developer Mode is needed. Normal use does not require
administrator rights. `latest.json` and `SHA256SUMS` are update and verification metadata.

For a manual upgrade, save your edits and choose **Quit** from the tray before
running setup. If Windows reports the app is in use, you can quit it and select
**Retry** in setup. Install the newer version over the existing one to keep your
presets and settings. Preview builds appear as **crosshair-rs Preview** in Start
and Game Bar and use separate settings from releases.

For a source build, see [Contributing](CONTRIBUTING.md#development).

## Fullscreen with Game Bar

1. In **Settings > Display**, choose **Game Bar**. If the widget needs setup,
   Desktop stays active and the editor shows the next step.
2. Click **Open widget**. If the toolbar is hidden, press **Win+G**. You can
   also open **crosshair-rs** (or **crosshair-rs Preview**) from **Widgets**.
   Pin it on your target display and turn on **Enable click-through**.
3. Press **Win+G** again. When the editor shows **Ready to switch to Game Bar**,
   click **Use Game Bar**. To switch back, choose **Desktop** in Display mode.
   You can leave the widget pinned for next time.

Desktop mode covers normal windows and borderless games. Fullscreen support
depends on Game Bar and the game; Vulkan/OpenGL fullscreen can be restricted.
Games and anti-cheat systems may block overlays. crosshair-rs does not inject
into games or bypass those restrictions.

## Updates

Open **Settings > Updates**, check for updates, then choose **Download**.
After verification, choose **Install and restart** and approve the Windows prompt.
The overlay and editor close during installation. Your settings are retained.

Checks use the latest stable release by default. Enable **Include prereleases**
to also receive alpha, beta and release-candidate versions. Their full version
names appear in the updater. Turning it off never downgrades your installation;
check again to see the available stable version. Automatic checks can be disabled
in Settings; downloads and installation always require your action.

Versions before alpha 6 need a manual setup upgrade to get this switch.

Alpha 1 used the earlier multi-file installer. Install a newer alpha with its setup
EXE before using the new update format; your settings are retained.

Before uninstalling, [export your data](data-and-privacy.md). Windows removes the
installed app's local data on uninstall.
