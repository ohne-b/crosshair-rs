# Troubleshooting

| Problem | What to check |
| --- | --- |
| Crosshair is hidden | Check the header switch, selected preset, monitor and position in Settings. |
| Missing in fullscreen | Use [Game Bar mode](installation.md#fullscreen-with-game-bar), pin the widget and enable click-through. Try borderless mode if the game blocks it. |
| Shortcut changes the crosshair while typing | Shortcuts are global. Change or remove the binding in Keybinds. |
| Editor lost its connection | Keep unsaved work open. Reconnect, then retry saving or export the draft before closing. |
| Saved crosshair changed while disconnected | Export the draft before choosing to replace the saved design. Replacement overwrites that design. |
| Editor freezes or stutters | Record the action, layer count, image dimensions, canvas zoom, Windows scaling and app version. Include those details in a bug report. |
| Data is already in use | Quit crosshair-rs in the other Windows session. Do not delete its lock or start another writer. |
| Settings need recovery | Use **Restore backup**. Preserve the data folder before resetting or replacing files. |
| Update fails | Open the information icon for details. Keep the current installation and retry, or use the [release installer](installation.md#install-a-release). |
| Installer rejects the download | Download all required files from the same release again. Do not skip signature or identity checks. |

When [reporting a bug](https://github.com/ohne-b/crosshair-rs/issues), include the
version, Windows build, display mode and steps to reproduce it. For game-specific
issues, include the game and its window/fullscreen mode. Remove personal names,
paths and private images from screenshots or shared files.
