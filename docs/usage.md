# Editor and shortcuts

## Crosshairs

Create a crosshair in **Crosshairs**, then open it in **Designer**. Add crosshair,
shape, image or drawing layers from the bottom palette. The Image tool opens an
import picker. Rename crosshairs inline and star favorites to find them again.

Designer changes preview immediately. Choose **Save** or press **Ctrl+S** to save
the design. Closing or opening another document with changes offers
**Save**, **Discard** or **Cancel**. A lost connection keeps your unsaved draft;
reconnecting does not save it automatically.

Drag layers to reorder them. Use the eye to hide a layer and the trash can to
delete it. Backspace deletes selected layers outside text fields. Drag numeric
fields horizontally or type exact values; geometry and moves use whole pixels.
Click a color swatch for HSV and palette controls; edit hex and opacity in its row.
Undo/redo groups each drag or stroke into one step.

Scroll over the canvas to zoom; hold the middle mouse button to pan. Canvas zoom
does not change overlay size. Output is limited to 1024 x 1024 pixels including
outline and blur. **Share** exports the current draft as transparent PNG or SVG.
SVG keeps shapes and text as vectors, embeds images at output resolution and
outlines as pixels. Blur and image sampling can differ slightly. SVG files are limited
to 16 MiB; use PNG for larger image-heavy designs.

Profiles group crosshairs and shortcuts. Choose a profile in the header and manage
them under **Settings > Data**.

## Shortcuts

Open **Keybinds** and choose **New**. Search the crosshair picker or use
**All**/**Favorites**, then click a card and record a keyboard or mouse shortcut.
Each row has an editable shortcut name, a thumbnail for changing the target and a
trash control. Preset shortcuts select on press; keyboard, mouse buttons and
mouse-wheel input are supported. Escape cancels recording.

Shortcuts work in every app, including while typing. The same input still reaches
the foreground app. There is no game detection or automatic profile switching.

Use the header power button to show or hide the crosshair. Its tooltip shows the
current shortcut; the default is **Shift+Alt+Z** on a US keyboard layout. Change it
in the application row in **Keybinds**. Older press/hold and preset-toggle bindings
are removed when loading; saved crosshairs and ordinary shortcuts are retained.

## Discord Rich Presence

Enable **Settings > General > Show activity on Discord** to share activity on
your Discord profile. **Simple** shows "Overlay active" or "Overlay hidden".
**Quote** shows "I've got you in my sights." and "— Soldier: 76". Both show the
app logo, **Download** and **Source code** buttons; crosshair names are not shared.

Discord desktop must be running with Activity Sharing enabled. The connection
continues with the editor closed and reconnects if Discord restarts. Turn the
setting off or quit the app from the tray to clear the activity.

## Closing and startup

Closing the editor leaves the crosshair running after resolving unsaved changes.
Reopen it through the tray's **Open menu** action. **Quit** stops the whole app.

**Settings > General > Launch at sign-in** starts the overlay without opening the
editor. Adjust the monitor, position and output scale in **Settings > Display**.
