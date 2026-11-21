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

Drag a shape corner to change its width and height. The opposite corner stays
in place, including on rotated shapes. Hold **Shift** to keep the proportions.
On a crosshair, drag a corner to change the gap while keeping the arm length and
thickness. If the arms already touch when you start dragging, the whole crosshair
scales. On a dot-only crosshair, the same gesture changes the dot diameter.
At 100% zoom, one design pixel is one screen pixel. At 800% and above, each
grid cell marks one design pixel.

Scroll over the canvas to zoom; hold the middle mouse button to pan. Canvas zoom
does not change overlay size. Output is limited to 1024 x 1024 pixels including
outline and blur. Choose **Share → Editable preset (.json)** to export the current
draft, including unsaved layer and animation edits. Use **File → Import** to add
it to your library as an editable preset. **Share** also exports transparent PNG
and SVG images.
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
in the **Global toggle** row in **Keybinds**.

The **Global toggle** shortcut, header button and tray toggle all control the
same master switch. Clearing its shortcut does not turn the overlay off.

In **Keybinds**, click **Unbound** in the **Hold to hide** row above **Movement**
and press the key you use to open your map, such as **G** in Rust. The crosshair
hides while you hold that shortcut. Releasing it restores visibility according
to the master switch and Aim settings. The shortcut belongs to the active profile
and accepts keyboard chords and mouse buttons.

**Aim**, **Fire** and **Reload** belong to the active profile. Their initial inputs
are Right mouse, Left mouse and R; an existing conflicting shortcut is kept and
the new action is left unbound. Aim and Fire accept keys or mouse buttons; Reload
also accepts the mouse wheel. These inputs still reach the game or other app.

Aim starts at **No action**. Choose **Hide crosshair (Hold)** to hide while holding
Aim, or **Show crosshair (Hold)** to show only while holding it. **Toggle crosshair
(Press)** changes the master switch. Both saved-crosshair actions open the picker
immediately: choose a Saved design, then hold or press Aim to use it. Cancel keeps
the old setting. Clicking the small thumbnail changes the target.

For example, choose **Show saved crosshair (Hold)** and pick a small dot. Holding
Right mouse shows the dot; releasing returns to your current crosshair, including
a Browse design. Choosing another crosshair while holding Aim updates what you
return to. A normal selection clears a press-toggled alternate.

Master Off always hides the overlay. Fire, Reload and saved-crosshair actions do
not turn it on. If you enable it while holding a trigger, release and press again.

## Animations

Select one layer in Designer and choose **Animate** at the top right of the
inspector. **Design** holds its normal Base values. Animate sets its target values;
untouched properties follow Base. Reset beside a changed property restores that
inheritance. The Enabled switch keeps an effect configured without playing it;
**Remove animation** removes it. Changes use normal Save and undo.

The **Aim / Fire / Movement** tabs each have their own animation and Enabled switch.
Switching tabs preserves the settings. Numeric changes combine relative to Base:
a gap of 4 with Movement targeting 7 and Fire targeting 6 becomes 9 together.
Releasing either trigger leaves the other effect active. Colors blend with Fire
above Aim above Movement. New animations start disabled.

For an effect while moving, open the **Movement** tab, enable it and choose Activation **Hold**,
then increase the Animated gap. **Move forward/left/backward/right** in Keybinds
use W/A/S/D by default and can be recorded separately. The effect stays active
until every movement key is released; **Move** previews it in Designer. Opposing
keys also keep it active. This responds to keys, not the game's actual movement
speed or weapon accuracy.

For a simple firing effect, choose a crosshair layer, open the **Fire** tab, enable it and
choose Activation **Hold**, then increase its Animated gap. The default 150 ms duration
moves toward that gap while Fire is held and returns smoothly on release. The
**Fire** preview button lets you try it locally; **Stop** ends preview.

Set Duration to **0 ms** for an instant change after any Delay. With Hold and
Return, it jumps to Animated on press and back to Base on release.

**Press** plays once per press. In **Timing**, Back and forth returns to Base as
part of the movement; Repeat loops while held, or toggles looping on successive
presses. Hold's On release choices are Return, immediate Reset or Pause. A new
hold resumes Pause. Delay applies before a fresh start, not each repeat.

Reload restores all effects on the current crosshair, including paused ones;
release held triggers before starting again. It does not change the selected
crosshair or visibility. Aim/Fire/Movement animations work with the dashboard closed.
They respond to your inputs, not a game's actual spread, recoil or ammunition.

PNG/SVG exports and thumbnails show Base. Saved crosshair documents, layer copies
and profile backups keep the animation settings. Existing designs remain static
until you add an effect.

## Discord Rich Presence

**Settings > General > Show activity on Discord** is on by default and can be
turned off anytime. **Simple** shows "Overlay active" or "Overlay hidden".
**Quote** shows "I've got you in my sights." and "— Soldier: 76". Both show the
app logo and a **Download** button; crosshair names are not shared.

Discord desktop must be running with Activity Sharing enabled. The connection
continues with the editor closed and reconnects if Discord restarts. Turn the
setting off or quit the app from the tray to clear the activity.

## Closing and startup

Closing the editor leaves the crosshair running after resolving unsaved changes.
Reopen it through the tray's **Open menu** action. **Quit** stops the whole app.

**Settings > General > Launch at sign-in** starts the overlay without opening the
editor. This requires the installed app; open it from Start. The switch reflects
Windows' startup setting. If Windows has disabled it, enable crosshair-rs in
**Windows Settings > Apps > Startup**. **Unavailable** means Windows' setting
could not be read. Adjust the monitor, position and output scale in
**Settings > Display**.

Setup offers desktop and additional Start menu shortcuts, launch at Windows
sign-in, and launch after installation. Windows also creates its standard Start
entry. Sign-in starts off on a new installation; leaving its setup checkbox
unchecked preserves an existing preference. Unchecking a shortcut removes only
the matching shortcut created by setup. Extra shortcuts can remain after
uninstalling the package; delete them manually if needed.
