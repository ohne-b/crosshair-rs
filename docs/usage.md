# Editor and shortcuts

## Crosshairs

Create a crosshair in **Crosshairs**, then open it in **Designer**. Add crosshair,
shape, image or drawing layers from the bottom palette. Open the **Image** tool
to drop an image into its popover or choose **Browse files**. Rename crosshairs
inline and star favorites to find them again.
In **Saved**, choose **Import** and drop a `.json` or `.crosshair` file onto
the popover, or choose **Browse files**.

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
On a crosshair, drag a corner to change the gap in whole-pixel steps. If the arms
touch, drag outward to reopen the gap. Arm length and thickness stay the same.
On a dot-only crosshair, the same gesture changes the dot diameter.
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

Open **Keybinds**. The monitor column shows keyboard and mouse shortcuts. The
gamepad column shows controller shortcuts. Click a cell to record that input.
To switch to a saved crosshair, choose **New**, pick the crosshair, and record
the shortcut in its row. Hover or tab to a binding to reveal its **×** button.
Click **×** to clear that assignment, or press Backspace while recording.
Escape cancels recording. A saved shortcut stays in the list even when both
assignments are empty. Its far-right trash button deletes the shortcut.

Controller bindings start empty. You can record a button, squeeze a trigger or
push either stick in a direction. For example, bind **Hold to hide** to the
left trigger to hide the crosshair while you squeeze it. Release the trigger
to restore it. A binding works on any connected gamepad that SDL recognizes,
including one connected after the editor closes.

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
to the master switch and Aim settings. The shortcut belongs to the active profile.

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
then increase the Animated gap. In **Keybinds**, the **Movement** grid starts with
W/A/S/D under **Forward**, **Left**, **Back** and **Right**. To bind a stick
direction, click **Unbound** in the Controller row under that heading and move the
stick. Click an existing binding to replace it. The effect stays active until
you release all bound movement inputs, even if you hold opposing directions.
In-game speed and weapon accuracy have no effect on the animation. Use
**Move** to preview it in Designer.

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
app logo. **Free Download** opens GitHub Releases and **Join Discord** opens the
community invite. Crosshair names stay private.

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

Windows adds crosshair-rs to Start. Setup offers a desktop shortcut, launch at
Windows sign-in, and launch after installation. Sign-in starts off on a new
installation. Leaving it unchecked keeps your existing preference. Unchecking
the desktop shortcut removes the matching link created by setup.

To keep your settings when updating, run the newer installer over the existing
installation. Before uninstalling or switching between Preview and release,
export all your data from Settings so you can restore your profiles and shortcuts.
