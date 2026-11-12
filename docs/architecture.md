# Application design and implementation plan

This defines the application architecture and its complete acceptance contract.
Source implementation does not establish compatibility or performance: native
acceptance, signed installation and update qualification remain separate gates.
Contributor and release policy lives in [CONTRIBUTING.md](../CONTRIBUTING.md).
The [design preview](design-preview.html) illustrates the proposed interface;
its interactions are illustrative, not an implementation of the application.

## Decisions and scope

| Area | Decision |
| --- | --- |
| Platform | Windows 10 22H2 and Windows 11, x64. |
| Editor | Rust + compiled Slint, Winit backend, software renderer initially. No browser runtime. |
| Engine | Independent Rust process: state, input, tray, persistence, rendering, presentation and updates. |
| Desktop overlay | Compact, nonactivating, click-through Win32 layered window. |
| Fullscreen overlay | Pinned Xbox Game Bar widget, where Game Bar supports widget presentation. |
| Widget host | Thin C++/WinRT UWP XAML executable using Microsoft's Game Bar SDK. It displays engine-produced pixels. |
| Crosshair rendering | Shared Rust renderer: tiny-skia, Windows DirectWrite text outlines, bounded image/SVG decoding. |
| Communication | Local authenticated named pipes; versioned JSON control and bounded binary widget frames. |
| Storage | Engine-owned versioned JSON and managed local image assets. |
| Installation | One signed MSIX containing editor, engine and widget. No Windows service. |
| Updates | TDM's Settings interaction and signed latest.json contract, adapted to MSIX. |
| Input | Global keyboard and mouse bindings, press/hold actions. No controllers. |
| Appearance | Orange placeholder logo, Tabler outline icons, Manrope interface font. |
| License | PolyForm Noncommercial 1.0.0; third-party components retain their licenses. |

The complete scope includes layered static editing, crosshair arms, primitive
shapes, images, freehand drawing, outlines, blur, local presets, manual
profiles, global visibility, keybindings, tray lifecycle, fullscreen integration,
import/export, updates, recovery, accessibility and packaging. The milestones
sequence this work; they do not make required features optional.

Exclude animations and animation stages, controllers, game detection, automatic
profile switching, foreground-process monitoring, cursor replacement, randomizer,
community feeds, workshop, shop, accounts, cloud sync, telemetry, Discord
integration and streaming servers. These are exclusions, not empty navigation
entries or a future-feature backlog.

### Changes from the original baseline

Slint replaces Tauri/React. Software rendering is the starting choice for this
mostly static interface, not a claim that it always outperforms GPU rendering.
Measure large-canvas interaction before fixing the release renderer. Enable only
the selected backend/renderer; do not ship multiple rendering stacks by default.
[Slint documents the available Winit renderers][slint-renderers].

A topmost window cannot promise fullscreen coverage. Game Bar adds a presentation
path, but Microsoft documents fullscreen limitations, including Vulkan/OpenGL
cases. Seeing the Game Bar shell is not sufficient: our pinned widget, position
and input must also pass compatibility tests. Describe support as
**Game-Bar-supported fullscreen**, never all exclusive-fullscreen applications.
No game injection or anti-cheat bypasses. [Microsoft's limitations][gamebar-limits]
define the boundary.

The widget is the deliberate exception to Rust-only native code. Microsoft's
documented widget model uses UWP XAML, with C++/WinRT samples. Its code only owns
activation, connection, pixels and widget lifecycle, never a second editor,
renderer, keybinding system or settings store. Put all three executables in one
package to simplify activation and communication. [Microsoft's packaging/IPC
guidance][gamebar-ipc] supports this arrangement.

## Reference inspection and visual direction

The running Crosshair X application was inspected directly: shell, saved library,
designer, layer operations, crosshair inspector, primitive tools, freehand,
straight/circular text, image-import dialog, action bindings, preset shortcuts,
profiles, settings and animation/cursor/randomizer surfaces needed to establish
exclusions. Temporary changes were discarded; the original saved crosshair and
off state were restored. No reference artwork or personal preset names belong
in this repository.

Image import was inspected without importing a file. Ellipse/triangle-specific
inspectors and every font/style variant were not exercised. The controls below
are our product contract, not claims about unseen reference behavior or its
internal representation. Its private file format is not an import target.

TDM reference revision: `736f6d20faccb6a194979928f0204a5ebec5e06c`.
Sources: [settings updater][tdm-ui], [update state/provider][tdm-state],
[shared controls][tdm-controls], [styles][tdm-styles], [verification][tdm-verify]
and [release script][tdm-release]. Match its compact settings rhythm, help
popovers and update states; adapt its backend to the native architecture.

### Shell and styling

- Default 1280 x 800 logical pixels; minimum 960 x 640. Use a fixed 56 px left
  icon rail, with labels on hover/focus. On smaller available work areas, collapse
  the layer list and scroll the inspector. Never force a window beyond the work area.
- 48 px custom title bar: logo alone above the left rail; manual profile picker, active-preset
  thumbnail, a 64px visibility switch with the configured chord in its tooltip,
  and native Windows minimize/maximize/close buttons right. DWM owns their drawing,
  hit testing and Snap behavior inside the custom frame. No On/Off text or profile
  overflow button; profile management lives in Settings. The rail has no logo separator.
- Navigation: Crosshairs, Designer, Keybinds. Bottom left: **only** Settings and
  GitHub linking to `https://github.com/ohne-b/crosshair-rs`. No sidebar updater
  banner, social links, news or promotional tiles.
- Designer: 44 px document toolbar, 210 px floating layer list, flexible canvas,
  288 px inspector. The layer list sizes to its contents and scrolls when needed;
  narrow mode retains layer access. Pair related numeric fields and keep section
  headings left aligned. Inspector and preview use the same document model.
- Colors: canvas `#171717`, preview `#141414`, panels `#1E1E1E`, fields `#252525`, raised `#292929`,
  hover `#333333`, divider `#303030`, text `#F4F4F5`, muted `#A1A1AA`, orange
  `#F97316`. Orange-filled controls use dark text for contrast.
- Manrope: 13 px controls, 12 px secondary values, 16 px section titles;
  400/500/600 weights. Tabular numeric values. Spacing 4/8/12/16/24 px, 8 px control
  corners, 10-12 px panel corners, 32-36 px controls, 16-18 px Tabler icons. No gradients, glow, giant
  cards, decorative statistics or welcome copy.
- One quiet selected state. Organize sections with spacing/dividers rather than
  nested cards. No descriptive paragraphs in Designer, library or Keybinds.
  Settings may have necessary short descriptions; long help uses a focusable
  `i` popover with keyboard activation, Escape/outside dismissal and focus return.
- Settings has Display, General, Data, Updates and About categories, with compact
  aligned rows. The library uses the same narrow category column for All/Favorites.
- Retain native drag/resize, double-click maximize, system menu, Alt+F4, Win+arrow
  and Windows 11 Snap. Use Slint public native-window handle access and one small
  Win32 frame adapter; no Slint fork. Test maximize hit-testing and DPI changes.
- Keyboard navigation, visible focus through control backgrounds (outlines only in text fields), screen-reader roles/names/values, high
  contrast and Windows text scaling are required. Icons have accessible names
  and focus/hover tooltips. Color is never the only signal. Honor reduced motion;
  no perpetual decorative animation.

Bundle only used [Tabler MIT icons][tabler-license] and Manrope with its license;
no runtime CDN requests. Use Slint's royalty-free desktop license and retain its
terms/required attribution. PolyForm does not permit silently selecting Slint's
GPL route. Add the official Slint badge to the public download README when Slint
is introduced, plus notices in Settings > About. [Slint's terms][slint-license]
describe the available choices.

## Screens and behavior

### Crosshairs and profiles

Local thumbnail grid: search, All/Favorites with counts, a compact icon sort picker
for Manual/Name/Recently edited, New and Import. Names rename in place on Enter or
focus loss; Escape cancels. Favorite and trash appear on hover or keyboard focus;
filled favorites stay visible. Right-click/Shift+F10 opens the compact card menu:
Edit, Rename, Duplicate, Export, Create keybind, order and Delete. Menus and help
popovers anchor to their controls, dismiss outside/on Escape and return focus.
No discovery tabs.
Single-click activates; double-click/Edit opens Designer. Manual order supports
drag and keyboard alternatives. Cache thumbnails and virtualize larger lists.
Ship a small original preset set, never reference artwork.

New creates a selected four-arm preset. Duplicate creates independent IDs while
sharing immutable assets. Delete offers Undo. If a preset has references, one
confirmation identifies affected bindings/profiles and the replacement; apply
atomically. Always retain one valid preset and profile.

A preset is a design. A profile groups shortcuts, selected preset, output scale,
offsets and saved positions. The preset library is shared: edits change that
design wherever selected; Duplicate makes an independent variant. Output mode
and monitor are app settings. The title-bar profile picker shows the active
profile; Manage offers Create/Rename/Duplicate/Import/Export/Delete. No game field.
Switch profile atomically, clear hold overrides and preserve global visibility.

The title switch, tray toggle and default remappable `Shift+Alt+Z` action operate
the same visibility state. The binding can be disabled. Passive observation does
not reserve OS/app shortcuts; explain collisions in keybinding help and never
modify Windows shortcuts automatically.

### Designer

Toolbar: editable document name, File, undo/redo, duplicate/delete selection.
Edits apply immediately and autosave; no Apply button. `Ctrl+S` flushes pending
state and reports failures rather than gating preview. File offers New, Import,
Export preset, Export PNG and Duplicate preset.

The layer list has New layer, ordered names, visibility and per-row trash.
Rename from the row; reorder by drag with an insertion marker. No footer menus
or move-up/down buttons. Legacy locks remain accessible from the context menu. Context menu: Copy/Cut/Paste,
Duplicate, Move up/down, Bring to front, Send to back, Delete. Locked layers stay
selectable from the list but immutable. Multi-selection transforms/duplication
do not require a nested group model.

Canvas: middle-button pan, pointer-anchored zoom, Fit and 100%. The dark background
is fixed and the design-pixel grid appears automatically from 800% zoom.
Panning uses screen-space deltas independent of the changing design transform.
The shared renderer rasterizes previews at display/zoom resolution, capped at a
1024px bitmap dimension; the engine still rasterizes at physical output scale.
Bounds/handles and the grid are editor adornments only.
Drag moves; handles resize/rotate; Shift constrains proportions/angles. Arrow
keys nudge one design pixel, Shift+arrow ten. Exact fields cover every transform.

Bottom tools: Crosshair, Shape (Rectangle/Ellipse/Triangle), Image and Draw.
Text creation is removed. Existing/imported text remains readable, renderable
and editable so the simplification never destroys saved documents.
No Design/Animate split. Inspector contains only the selected type's controls.
Lines/Dot/Outline collapse independently; remember expansion during the session.

| Layer/control | Static behavior |
| --- | --- |
| Common | Name, visible/locked, X/Y, uniform scale, rotation, opacity, nine-point transform anchor. Changing anchor preserves visual position. |
| Crosshair arms | 1-32 equally spaced arms, individual arm enable toggles, length, thickness, gap, rotation, fill/opacity and line blur. Four-arm default can omit any arm. |
| Arm shape | Rectangle, triangle, inward/outward chevron, arc, T-shape or image. Existing text arms retain their controls. Reuse asset controls and show only relevant geometry. |
| Center dot | Enable, square/circle, diameter, fill/opacity and blur. |
| Crosshair outline | Enable, color/opacity, thickness and blur. Include arms/dot without accidentally filling the gap. |
| Rectangle | Width/height, corner radius, fill, outline, blur, common transform. |
| Ellipse | Width/height, fill, outline, blur, common transform; transparent fill makes a ring. |
| Triangle | Width/height, fill, outline, blur, common transform. |
| Legacy text (existing/imported only) | Content, installed family, available weight/style, size, spacing, fill, outline, blur; straight/circular layout, radius, Repeat to fill. |
| Image | Import, crop, dimensions/aspect lock, opacity, rotation, blur, alpha-silhouette outline; nondestructive crop retains source. |
| Freehand | Color/opacity, brush size; left-button draw, right-button erase, Escape exits. Each stroke is one history step; layer transforms/outline/blur apply. |

Arm-specific fields are chevron depth/direction, arc radius/sweep and T crossbar
width/thickness; rectangle/triangle use arm length/thickness. Text/image arms
repeat their resolved content around the selected arm positions. Circular text
starts at a controllable angle with tangential glyph orientation; Repeat to fill
computes a bounded repeat count from shaped advances and circumference, never an
animation. Freehand retains vector strokes and eraser strokes; erasing affects
only that layer, never lower layers. These use the same common effect order.

Swatch/hex/opacity share a compact color control. Sliders pair with directly
editable numeric values, stepping and keyboard input. Invalid/nonfinite values
keep the last valid result; typing intermediate values does not jump the cursor
or clamp prematurely. Zero-size components produce empty output predictably.

Undo/redo covers edits, import, delete and ordering. A drag/slider gesture/text
edit is one history entry. Cap history at 32 MiB, targeting at least 100 ordinary
edits; reference immutable assets instead of copying them. `Ctrl+Z`, `Ctrl+Y`,
`Ctrl+Shift+Z`, clipboard shortcuts, `Ctrl+D` and bracket ordering work. Escape
cancels a transform to its prior valid state. Text fields retain text shortcuts.

Pin the editing document ID. A global key may switch the overlay while typing,
but never redirect edits to another preset. Header shows the actually active
preset; a compact Use action appears when editing an inactive one. Autosave is
quiet except on failure. Keep unsent edits after disconnect; do not replay them
over a changed revision. Closing with unsent edits offers Retry/Discard/Cancel;
acknowledged autosaved work needs no save prompt.

### Keybinds

Two compact tables: Crosshair shortcuts and Press & hold actions, with New rows
at the top, target thumbnails, key chips and trash controls. No enabled switches,
repeated Select crosshair labels, controller columns or animation rows.
One dedicated application row owns Toggle visibility; new profile actions offer
Hide (hold), Show (hold), Toggle preset (press) and Show preset (hold).
Recording an existing disabled shortcut enables it.
Preset shortcuts select a target on press. All rows belong to the active profile
except the clearly identified application-wide visibility chord.

Click a chip to listen: `Press a key`, highlighted background. Escape cancels, Backspace
clears, outside click cancels. Each capture has a unique token; delayed results
from a cancelled capture cannot finish a newer one. Wait for already-held keys/buttons to release so
the click opening capture is not assigned as Mouse 1. Support keyboard chords,
mouse buttons 1-5 and wheel up/down (wheel is press-only).

The engine captures; shortcut dispatch is suspended during capture, but input
continues reaching all other apps normally. Restore dispatch on commit/cancel,
editor disconnect or a 30-second capture lease expiry; wait for captured keys to
release before rearming. Identical effective chords show an inline conflict with
Replace/Cancel, never two racing actions.

### Settings

Use one quiet surface, headings and separators rather than card stacks.

| Group | Controls |
| --- | --- |
| Display | Desktop/Game Bar output, monitor, scale, X/Y, reset position, named saved positions; help behind i. |
| Game Bar | Installed/connected/pinned state, Open Game Bar, setup help and repin recovery. Only show relevant controls in that mode. |
| Startup | Launch at sign-in; Show crosshair on launch. Startup runs only the engine. |
| Interface | Windows text scaling/reduced motion respected; reset editor layout. Fixed dark design, no exposed rendering backend toggle. |
| Data | Import/export profiles/all data, open data folder, confirmed settings reset with backup. |
| Maintenance | Version, update check preference, manual check, inline status/progress, release notes, download/cancel/install. |
| About | Version, source license and third-party notices/attribution. |

Startup defaults off; show-on-launch defaults on. Visibility persists for crash
recovery, while explicit new launches follow the startup preference. Updates
restore pre-update visibility. Closing always exits the editor; there is no
ambiguous minimize-on-close option. Use Windows minimize to minimize.

## Ownership and folders

One Cargo workspace, two product Rust binaries, three shared crates and a small
SDK host. Create directories with working code, never empty future scaffolding.
Separate domain, platform and presentation like TDM without its web/Python stack.

```text
Cargo.toml                      # workspace version and shared dependencies
Cargo.lock
apps/
  engine/src/
    main.rs                     # normal, startup and update-helper modes
    app.rs                      # authoritative state/history/input model
    commands.rs                 # validated domain mutations
    runtime/                    # native orchestration and worker completions
    work.rs                     # bounded serialized storage/validation worker
    input/                      # Raw Input, device/key state
    overlay/                    # native output and widget connection
    ipc/                        # endpoints, framing, authentication
    storage/                    # JSON, assets, migration/recovery
    updates/                    # check/download/verify/deploy
    tray.rs
  editor/
    src/
      main.rs
      app.rs                    # Slint models and connection lifetime
      features/                 # library/designer/keybinds/profiles/settings
      platform/                 # frame, dialogs, activation
    ui/
      app.slint
      shell/                    # title bar and navigation
      features/                 # same screen boundaries as Rust controllers
      shared/                   # reused controls and tokens
  gamebar/
    CrosshairWidget.sln
    CrosshairWidget/            # SDK activation, pixels, lifecycle
crates/
  core/src/                     # IDs/documents/bindings/validation/protocol
  renderer/src/                 # geometry/text/assets/effects/raster/cache
  platform/src/                 # shared Windows identity and authenticated pipes
assets/
  logo.svg
  icons/                        # selected Tabler icons and license
  fonts/                        # Manrope and license
packaging/windows/              # MSIX identity/manifest/artwork
crates/renderer/tests/           # deterministic synthetic rendering fixtures
scripts/                        # actual build/QA/package entry points only
.github/workflows/              # validation, manual prepare/publish
docs/architecture.md            # canonical plan/contracts
docs/design-preview.html        # illustrative design, not app source
```

No plugin system, DI container, event-bus framework, generic service/repository
stack or crate per feature. Core has no Slint/window/filesystem dependency.
Renderer owns no input/persistence. Rust feature controllers adapt domain state;
Slint describes layout/interaction, not duplicate business validation.
The platform crate shares the same identity and pipe implementation between the
engine and editor; it contains no application state or rendering code.

Pin dependency versions when introduced and commit the lockfile. Baseline:
`windows`, `serde`/`serde_json`, `tiny-skia`, `slint`/`slint-build`, `image` with
only PNG/JPEG/GIF codecs, `resvg`/`usvg`. Use DirectWrite through `windows` for
system fonts/shaping, Windows HTTP APIs for downloads, maintained Minisign and
SemVer implementations. Reuse suitable existing primitives. No handwritten
cryptography, TLS client or async runtime just to keep an idle message loop alive.

## Data and editing contract

Stable opaque IDs identify presets/layers/profiles/bindings/assets. Names are
editable labels, never identity or filesystem paths.

| Entity | Contents |
| --- | --- |
| App state | Schema version, settings, profiles/active ID, preset library, asset index, base selection/visibility, revisions. |
| Profile | ID/name, selected preset, bindings, scale/offset, saved positions. |
| Preset | ID/name, ordered layers, favorite, manual order, document revision and engine-owned global edit order for recency. |
| Layer | ID/name, visible/locked, transform, opacity, typed content/effects. |
| Content | Crosshair, rectangle, ellipse, triangle, text, image or freehand strokes. |
| Binding | ID, chord, press/hold action, target if needed, enabled. |
| Runtime only | Pressed devices/keys, holds, capture, connections, caches, edit history and update state. |

Geometry uses physical design pixels. UI logical pixels/display scale never
silently change thickness. Profile scale is an explicit transform; offsets are
physical output pixels. Values are finite/bounded; serialization does not depend
on enum memory layout or pointer size.

One engine owner serializes mutations. Commands carry target ID, expected
document revision and request ID. Acknowledgements publish revisions/events.
Reject stale edits with current state for resolution. Separate selection/settings
revisions from document revisions so a preset-switch key cannot invalidate an
unrelated edit.

Editor previews may be optimistic using the shared renderer; the engine validates
every authoritative update. Coalesce drag/slider updates to newest once per
display frame; always send final release. Begin/update/end transactions produce
one undo entry; cancel reverts that transaction. Lost connections retain the
last acknowledged valid state rather than rolling back unrelated work. Bound
transaction lifetime/history. Keep local unsent edits for deliberate reapply,
never silently overwrite a changed document.

### Imports and limits

Preset/profile/all-data exports use versioned self-contained JSON with embedded
assets. PNG exports transparent rendered output. Imports validate the whole
graph, create/remap IDs and merge once; all-data Replace requires explicit backup
and confirmation. No partially applied imports or executable content.

Accept PNG/JPEG/static SVG and GIF's first frame, labeled static in the dialog.
Normalize orientation/color handling to documented sRGB premultiplied pixels.
SVG parsing is bounded and static: no scripts/network/external files/fonts or
unrestricted data URLs. Reject unsupported references; preserve supported vector
source for resizing. Never load imported SVG in a browser.

Text retains editable content/family/style. Missing fonts require an explicit
fallback choice without changing the stored family silently. Export does not copy
installed fonts. Bundle Manrope for deterministic fixtures; test Unicode/fallback
with DirectWrite. Unsupported color-font content produces a clear error or an
explicitly chosen monochrome fallback.

Enforce bounds centrally in core and reuse them for UI/import/IPC:

- 64 layers/preset, 500 presets, 32 profiles, 256 bindings/profile.
- Final bitmap at most 1024 x 1024 physical pixels including blur/outline.
  Position offsets do not enlarge it; oversize output is an error, not clipping.
- Source image 8 MiB, decoded 16 megapixels/64 MiB, one job scratch budget 128 MiB.
- 1024 text characters/layer; 20,000 freehand points/preset.
- Non-asset configuration at most 8 MiB; portable JSON at most 512 MiB including
  base64 and metadata. Enforce that aggregate portable-size budget on the whole
  live library before accepting edits/imports, deduplicating assets by content.
  Thus every valid preset, profile and all-data backup fits the same restore
  limit. Stream import/export one bounded asset at a time through staging; never
  allocate a whole backup in memory. Bound nesting and SVG element/path complexity
  plus worker time/allocation, not just file bytes.
- Raster cache 32 MiB, decoded-asset cache 64 MiB, LRU eviction. Remove unreferenced
  stored assets only after durable saves and backup-retention checks; cache
  pressure never deletes a live source asset.

These prevent allocation spikes, not justify silent quality reductions. Test at
and above limits. Change them only with measured evidence and contract review.

## Rendering and presentation

One renderer generates overlay, previews and thumbnails. Slint draws the bitmap,
grid and handles, not another implementation of crosshair geometry.

Resolve geometry/text paths, transform about anchor, calculate padded bounds,
rasterize fill/outline, apply bounded blur, then composite in layer order with
premultiplied alpha in the same documented color space. Cache by document revision,
output scale, assets and relevant font identity. Preview zoom never alters design.

tiny-skia is not a complete text/image/effect engine. Use DirectWrite shaping and
glyph outlines, the selected image codecs and resvg. Implement only the required
bounded mask/effect operations; blur and alpha-silhouette outlines need golden
tests. [DirectWrite exposes glyph outlines through geometry sinks][directwrite].

Render only on relevant state/display changes. Prewarm shortcut targets that fit
the cache while keeping input dispatch responsive. Evicted complex targets render on demand
with newest-request-wins cancellation; never present an obsolete result. Benchmark
warm and cold switching. Integer-aligned edges stay crisp; fractional/rotated
geometry is antialiased intentionally. For odd thickness/even monitor centers,
choose a stable nearest-pixel placement with at most 0.5 px center error rather
than blurring a one-pixel line. Use identical rounding on both outputs.

### Desktop

Compact Win32 window: `WS_EX_LAYERED`, `WS_EX_TRANSPARENT`, `WS_EX_TOPMOST`,
`WS_EX_TOOLWINDOW`, `WS_EX_NOACTIVATE`. `UpdateLayeredWindow` consumes BGRA;
swizzle premultiplied RGBA once without double premultiplication. Exclude from
taskbar/Alt+Tab; preserve focus and input. Set Per-Monitor DPI Awareness V2 before
creating windows.

Place against selected monitor physical bounds, not work area/editor bounds.
Persist stable display identity, fall back to primary on disconnect and restore
preference when it returns. React to DPI/resolution/orientation/display events.
No full-desktop bitmap or continuous render timer. Do not fight secure desktop.

### Game Bar

Widget frames carry version/generation, dimensions/stride, premultiplied BGRA,
position and visibility. Validate lengths/overflow and use noninterpolated
presentation. Prove physical-pixel mapping through widget logical coordinates
and DPI. Coalesce to newest generation; no base64 image JSON or shared memory
unless actual measurements justify it.

Users enable Game Bar, open/pin the widget and enable click-through. Use documented
APIs and Settings help, never private Game Bar settings manipulation. Transparency,
chrome removal, opacity, centering, monitor choice, pinned visibility and focus
are mandatory proof tests; a CenterWindow API name does not prove pixel accuracy.

Only one output shows a crosshair. Mode changes are explicit, never based on game
detection. Prepare/acknowledge the destination before hiding the old output; keep
the old mode if setup fails. No double-drawing or false fullscreen-success state.
Widget blanks on engine disconnect/Quit; it cannot retain stale pixels. Reconnect
with bounded backoff only while requested/pinned, without fabricated activity to
defeat UWP lifecycle. Report disconnected/unpinned status. Editor close never
closes the widget. Desktop mode remains usable without Game Bar.

Output changes use a bounded native-owner transition. A hidden preparation frame
and a visible activation frame have different generations; acknowledgments must
match both generation and actual bitmap state. Shell visibility alone is not an
acknowledgment. Prepare the desktop bitmap while hidden before requesting a widget
blank. Settings, all-data import and reset/recovery use the same boundary, and
durable publication stays on the storage worker. Continue observing global input
while work runs; changes made during the operation survive its publication.
Queued timeout messages cannot cancel an already committing state change.

If a failed activation cannot acknowledge blanking, leave the desktop hidden and
report that the widget must reconnect to confirm it has cleared. A broken pipe
alone does not prove stale pixels disappeared. The blank request remains the
latest frame across reconnection. Normal transitions never draw both outputs.

## Global input

Keyboard Raw Input: `RegisterRawInputDevices` with `RIDEV_INPUTSINK`, engine
`WM_INPUT`/`RAWKEYBOARD`. Register mouse only while mouse bindings/capture need it;
discard motion immediately. Wheel is a pulse, never held state. No
`RIDEV_NOLEGACY`, gameplay `RegisterHotKey`, suppression, injection, foreground
filtering or `GetAsyncKeyState` polling.

Represent physical scan codes, extended keys, optional sided modifiers and
exceptional keys explicitly; translate display labels without rewriting bindings
on layout change. Test AltGr, Shift sides, numpad, Pause/PrintScreen and multiple
keyboards. Track pressed state by device so one device cannot release another.

- Press actions fire on down edges, not repeats.
- Required modifiers are a subset: plain E still works while sprinting with
  Shift; a matching Shift+E wins over E. Choose the most specific chord and reject
  equally specific ambiguous overlaps. At most one action per event.
- Select changes base preset. Toggle preset selects its target or returns to the
  previous valid base when its target is already active.
- Newest still-held preset override wins; visibility holds resolve separately.
  Release reveals current base state, including changes made while holding,
  rather than restoring a stale snapshot.
- Visibility toggle changes base and cancels active visibility holds until their
  triggers release, so Off works immediately. A fresh Show-hold can reveal a
  hidden base temporarily.
- Profile change, removal, lock/suspend, lost input and capture cancellation clear
  affected holds. Rearm after release; lost key-up must not leave stuck state.
- Bindings work globally in the editor and every other foreground application.
  Only explicit capture suspends dispatch. Never log keys or foreground apps.

Test real delivery and 8 kHz mouse load. If necessary, batch Raw Input reads and
discard motion without allocation; never compensate by polling or dropping ups.

## IPC, storage and lifecycle

### Execution and IPC

One engine state/message thread handles mutation and fast input dispatch. A
bounded storage/validation queue and a cancellable latest-render worker keep
decoding and files off that thread. Native tray menus and save-error dialogs use
one separate UI thread so their modal message pumps cannot stall Raw Input.
The control server admits at most two clients, each with bounded reader/writer
transport work; the widget has one connection. Updates create a worker only while
active. Workers post results to the owner. No thread per preset, unbounded queue
or background scheduler. Slint's thread never blocks on I/O/large renders; post
newest results to its event loop.

Desktop pipe: current-user logon-session ACL, remote rejection, authenticated
peer identity. Names include user/session but are not the security boundary.
Prevent first-instance squatting; verify expected packaged executable identity.
Handshake verifies build/protocol before accepting commands. Control protocol v2
adds capture tokens to Begin/Cancel/Captured; both executables must be rebuilt together.

Length-prefixed UTF-8 JSON, 1 MiB control frames, incomplete-frame deadlines and
bounded requests. IDs/event revisions prevent stale responses replacing newer
state. Reconnect obtains a bounded immutable snapshot token, small library index
and requested documents in length-limited chunks, all tied to the same revision.
Expire abandoned snapshots; cap retained snapshots and assemble at most one
document up to the 8 MiB configuration bound. Apply later revision events only
after snapshot completion. Test valid state larger than one control frame and
mutation during transfer. No periodic GetState. Commands cover library/profiles,
capture/bindings, visibility/settings, imports/exports, updates, Open menu/Quit.
No generic filesystem/executable command endpoint. Large imports use engine-issued
staging tokens and bounded files; validate/open/copy before adopting managed assets.

Widget pipe is separate: connect, frame/status and visibility acknowledgement
only. AppContainer access needs documented package/capability SID handling and
package-local namespace plus session checks; a desktop user ACL alone does not
prove it works. Verify package/session; broad AppContainer identities get no
settings/update/shutdown authority. Prove the restricted endpoint in the first
packaged spike and test denial to another package/user/session.

### Durable state

Use per-user `ApplicationData.Current.LocalFolder`, never the install directory.
Engine writes authoritative state/assets; editor can write temporary staging only.
Because this folder is shared across the user's Windows sessions, acquire a
per-user lifetime storage lock as well as the per-session instance mutex. Only
one session for a Windows user may run an authoritative engine at a time. A second
session reports that the app is running in another session and asks the user to
quit it there; it never opens a second writer or migrates the store. A per-user
update coordination lock also protects deployment handoff across sessions.
Updates retain data; uninstall removes package data, so provide export. Freeze
package identity before release.

Debounce 300 ms, maximum two seconds during continuous gestures. Write complete
same-directory temporary snapshots, flush, atomically replace live JSON and keep
a known-good backup. Commit assets before references. Quit/update flush pending
state; failed flush blocks installation. A deliberate Quit with failed persistence
keeps the engine alive and offers Retry, Export recovery data, Cancel or explicit
Discard and quit. Headless tray Quit opens that recovery surface only on failure.
Forced OS termination cannot guarantee saving acknowledged RAM-only changes;
preserve the last durable snapshot/backup and report recovery on next launch.
Atomic rename alone is not a proven
power-loss guarantee: exercise interruption boundaries.

Explicit schema migrations have backups/tests. Unknown newer versions, corruption,
missing assets, full disk or denied writes never silently become defaults.
Preserve originals; offer recovery/export/known-good backup. Defaults only for
new installations or confirmed reset. Diagnostics are local, bounded and rotating;
no input contents/user paths by default. Explicit diagnostic export sanitizes
personal data and shows its contents. No automatic upload.

### Processes

Per-session mutex and authenticated handshake enforce one engine per session;
the additional per-user storage/update locks prevent cross-session writers. Launch
connects/starts engine, then opens/focuses one editor. Concurrent launches converge.
Editor close fully exits its process; engine/presentation continue. Editor crash
does not stop the crosshair.

Tray: Open menu, Show crosshair, compact active-profile preset submenu, Quit. Quit closes
editor, blanks/releases widget, unregisters input/tray, flushes and exits. No
orphan helper or respawn after deliberate Quit. An open editor offers Restart on
engine failure while preserving local edits; headless failures do not spawn a
surprise editor or a restart loop.

One packaged StartupTask, off by default, starts only engine mode. Honor Windows
DisabledByUser. Activation uses stable app IDs, not versioned WindowsApps paths.
Settings/recovery publication captures the actual Windows startup state, verifies
the requested state and compensates if reconciliation or durable publication
fails. No WinRT startup wait runs on the input thread.
Handle lock/unlock, sleep/wake, display change and Windows shutdown deliberately.
Normal use needs no administrator rights.

## Updates and release packaging

### TDM-style Settings experience

Engine owns update state across navigation/editor reopening. Default one quiet
check per editor launch, user-disableable; manual refresh remains available.
Headless engine never periodically contacts GitHub. No automatic download/install.

| State | Inline Settings behavior |
| --- | --- |
| Idle | Version chip and check icon. |
| Checking | Disabled check, concise checking status. |
| Current | Up-to-date status. |
| Available | Version, Update, Release notes; reserve progress space. |
| Downloading | Thin progress, bytes, Cancel; percentage only with reliable total. |
| Verified | Ready-to-install status, Install and restart. |
| Installing | Disabled actions; state that overlay is closing for installation. |
| Check/download error | Short inline error, Retry and release link. |
| Install error | Recheck installed state, actionable recovery/manual release link; no blind retry loop. |

Use TDM's compact rhythm, 6 px bar and tabular values, no wizard/card stack.
Unknown totals show bytes only. Cancel removes partial data and returns Available.
A Settings dot may mark availability; never add another sidebar footer item.

### Manifest and authenticity

Endpoint: `https://github.com/ohne-b/crosshair-rs/releases/latest/download/latest.json`.
TDM schema, with `windows-x86_64-msix` platform:

```json
{
  "schemaVersion": 1,
  "version": "1.2.3",
  "notes": "https://github.com/ohne-b/crosshair-rs/releases/tag/v1.2.3",
  "pub_date": "2026-11-11T12:00:00Z",
  "platforms": {
    "windows-x86_64-msix": {
      "url": "https://github.com/ohne-b/crosshair-rs/releases/download/v1.2.3/crosshair-rs-1.2.3-windows-x64.msix",
      "signature": "BASE64_ENCODED_MINISIGN_SIGNATURE_FILE"
    }
  }
}
```

Illustrative only, not a published release. Treat metadata as untrusted: small
size cap, strict schema/SemVer/HTTPS, exact repository/tag/asset validation and
bounded allowlisted GitHub asset redirects. Reject userinfo, unexpected ports,
query/path tricks. Notes are inert text/link, never HTML. Metadata timeout 20 s,
download deadline ten minutes, package cap 512 MiB enforced against actual bytes.

Stream to private staging, not a whole installer RAM buffer. Verify pinned
project-specific Minisign key and authenticated trusted-comment `version:VERSION`,
matching manifest and package identity/publisher/architecture/mapped version.
Otherwise old signed bytes could be relabeled as a new version. A verifier that
requires contiguous input may use a bounded read-only mapping; do not claim
streaming cryptographic verification without its support.

Minisign and Windows publisher trust are separate. Never install trust certificates
silently or reuse another project's private key. Rotate keys through a reviewed
trust transition, not a manifest-provided key. The implementation uses streaming
prehashed Minisign verification and native WinHTTP on a short-lived worker, with
an absolute request watchdog and cancellation. It has no idle networking thread.
Before handoff, WinVerifyTrust checks cached chain/revocation evidence without
starting background network requests. Missing trust evidence blocks installation
while the existing app remains usable; the release link provides manual recovery.

### Install and restart

Direct signed MSIX with OS PackageManager, no competing `.appinstaller` automatic
update association. [Microsoft supports code-driven non-Store updates][msix-update].
Full-trust desktop code deploys, never the widget. No Windows App SDK dependency
just for deployment.

1. Verify before stopping anything. Install and restart acquires update lock,
   flushes state, records target/restart intent and rejects further mutations.
2. Start a temporary engine copy in update-only mode in private staging. Accept
   only fixed package identity and verified local package; reverify before use
   and prevent replacement between check/deployment.
3. Coordinator must run outside package identity and survive package shutdown.
   Prove desktop process policy/identity; copying an EXE is insufficient. It must
   work without old versioned install paths or loading writable adjacent DLLs.
4. After takeover acknowledgement, close editor, blank/release widget, unregister
   input/tray, exit engine. The helper then snapshots matching editor/widget
   processes once, verifies their user/session/exact installed package identity
   and waits on their handles for at most 15 seconds. It never kills them or uses
   forced package shutdown; failure preserves the installed version.
5. Deploy local package and observe result. No downgrade flags. Query actually
   installed version, activate stable app identity and restore selection,
   visibility/output. Reopen Settings if editor initiated the update.
6. On failure reactivate the still-installed version with the recorded error.
   On interruption reconcile installed state/journal at startup. No duplicate
   relaunch mechanism. Remove staging after success/diagnosis.
7. Recheck widget connection/pin/placement. Their preservation through every
   reinstall/update is not documented; surface repinning help if needed.

Helper is a temporary engine mode, not a permanent service or second settings
writer. The first packaging spike proves shutdown survival, deployment failure
and restart. Resolve failures before depending on this updater; do not ship an
untested handoff.

### Packaging and publication

Freeze package name/publisher/app IDs/widget extension before first release.
Production trust and timestamped signing are release prerequisites. Test framework
dependencies on clean machines: C++ widget may need Microsoft VCLibs. Supply
trusted dependencies for offline distribution or state online acquisition
requirements. One application package does not imply zero runtime dependencies.
No separate Store-widget installation in the standard path. Game Bar itself is a
prerequisite only for its output mode. The package identity is
`OhneB.crosshair-rs`, with `Editor`, `Engine` and `Widget` app IDs and
`CrosshairOverlay` widget extension. Publisher is compiled from the production
certificate subject; `CN=ohne-b` is the unsigned development default, not a claim
of certificate ownership. The three executables are package-root siblings.

`scripts/package-windows.ps1` validates a real build and supplies an offline
`Microsoft.VCLibs.140.00` x64 **14.0.33519.0** dependency beside the MSIX. The
framework is extracted from Microsoft's hash-pinned VS2022 CRT redistributable
CAB and checked for exact hash, identity and Microsoft signature. `Install.ps1`
passes it to Windows as a dependency; it does not import certificates or change
Developer Mode. This is the UWP VCLibs framework, not `UWPDesktop`.

Freeze the Microsoft framework minimum versions for the first stable release
line. Before stopping the app, validate that installed dependencies satisfy the
new package. A release requiring higher framework versions must extend the
signed update contract with pinned Microsoft-signed dependency assets, acquire
and verify them before handoff, and supply them to deployment; otherwise block
in-app installation with an actionable manual-install link while the old app
continues running. Never discover missing dependencies only after shutdown.

Workspace version owns product version; derive/check Rust, widget and MSIX
metadata. Stable `A.B.C` maps to `A.B.C.0`. Local previews use the separate
`OhneB.crosshair-rs.Preview` identity and an explicitly monotonic fourth-component
`-PreviewBuild` number. They do not consume the stable update channel; in-app
updates are disabled. Release automation accepts stable versions only. Stable
latest never references a prerelease or becomes blocked by a higher preview.

Follow existing OhneGuessr/TDM conventions:

- Draft release PR changes version/changelog; merge and publication are separate.
  No publish on ordinary pushes/tags.
- Validate exact canonical main and retain immutable artifacts with version,
  commit/platform/SHA-256 metadata. Publisher consumes that revision's successful
  artifacts, not an older success after a failed/cancelled newer attempt or a
  silent rebuild. Expired artifacts require revalidation.
- Manual publication signs/verifies package, signs authenticated release version,
  generates latest.json/checksums. No signing secrets in PR jobs.
- Draft release gets MSIX, detached signature, latest.json and SHA256SUMS.
  Download/verify assets and complete clean-install/upgrade/recovery evidence
  before publication. Only stable advances latest.
- Title `crosshair-rs vVERSION`, one Changelog comparison footer, one issue link.
  Never overwrite published tags/assets. Broken releases get a new corrected
  version; preserve old config until recovery succeeds.

`validation.yml` retains immutable `windows-SHA-RUN-ATTEMPT` artifacts for 14
days. `prepare-release.yml` creates the version/notes draft PR;
`publish-release.yml` signs the selected exact-main artifact, verifies downloaded
assets and stops at a draft. Production public build variables and protected
signing secrets, local commands and the final manual acceptance boundary are
specified in [CONTRIBUTING.md](../CONTRIBUTING.md#maintainer-commands-and-signing-configuration).
Missing keys fail closed before any update request. Package signing, helper
breakaway survival, two-version deployment and clean-install behavior require
actual signed runtime evidence; source checks alone do not prove those gates.

## Performance and validation

These are proposed budgets, not results. Record CPU/cores/RAM, GPU/driver,
Windows/Game Bar versions, resolution/DPI/refresh, power mode and release SHA.
Measure warm/cold separately and report engine/widget/editor plus incremental
Game Bar cost separately, not a favorable engine-only figure.

| Scenario | Acceptance target / measurement |
| --- | --- |
| Static desktop, editor closed | No app render timer/periodic IPC/network; average engine CPU below 0.1% total machine capacity over a quiet 60-second trace. |
| Ordinary preset, engine only | Private working set target <=32 MiB after warmup; cache-heavy fixtures measured separately against caps. |
| Editor idle | No continuous rendering; private working set target <120 MiB with ordinary vector presets. |
| Cached switch | Input receipt to native submission p95 <5 ms; separately measure displayed frame at actual refresh rate. |
| Ordinary live edit | Preview/update p95 <16 ms; never decode/save on UI thread. Maximum-bound jobs stay cancellable/responsive. |
| Game Bar | Compare incremental CPU/GPU/memory against Game Bar alone, reject continuous app redraws; establish numeric budget from first real spike before release acceptance. |
| Stress | Rapid keys, 8 kHz mouse, max assets/layers, repeated open/close/mode changes: bounded memory/handles, no stale frames/lost ups. |

Use ETW/WPR or equivalent, release builds and repeatable synthetic fixtures.
Report p50/p95/worst, not a single Task Manager screenshot. Budget changes require
evidence and a recorded decision, not quietly removing a failed criterion.

| Owner | Required automated coverage |
| --- | --- |
| Core | Bounds/references/revisions, history, transactions, profiles, input/hold arbitration. |
| Renderer | Goldens for every layer/effect, channels/alpha, overlap, odd/even alignment, text/circular text, crop/SVG/blur, 100/125/150/175/200% cases. |
| Storage | Round trips/migrations/newer versions, corruption/assets, interrupted save/failed Quit, disk/access failures, aggregate export-size admission and full-limit backup/restore, two-session exclusion. |
| IPC | Oversize/truncated/timeouts, coherent multi-frame snapshot during mutation, stale/wrong build, reconnect and denied user/session/package identities. |
| Editor | Focus/capture, validation, pinned edit target, conflicts/reconnect, history/selection, preview generations, update states. |
| Updates | Bad manifest/redirect/signature/version/publisher, relabeled old package, cancel/full disk, interrupted helper/deployment/recovery. |
| Packaging | Derived version, files/notices, stable identity, signatures, install/upgrade fixtures. |

Add actual Rust fmt/Clippy/tests, Slint checks, MSBuild/package and Windows
integration commands to CONTRIBUTING with runnable components. Never claim
nonexistent tests pass. Keep screenshots/fixtures free of personal paths/presets.

Manual release acceptance:

- 1080p/1440p/4K, 100/125/150/175/200%, mixed DPI/negative coordinates/portrait,
  monitor reconnect, resolution/refresh change, sleep/wake. Capture actual pixels.
- Click-through/focus; windowed/borderless/Game Bar fullscreen with D3D11/D3D12
  and representative Vulkan/OpenGL. Record game/build/API/mode and actual widget
  outcome. An unsupported path is documented, not passed by calling it borderless.
- Real key/mouse delivery, modifiers/repeats/release, removal/lock, multiple
  keyboards, capture loss, global switches while typing, rapid profile changes.
- Every static control, circular text/import/drawing/erasing/effects, exact edits,
  undo after deletion/import, keyboard access/screen reader/high contrast/text scale.
- Cold/concurrent launch, editor/engine crash, Quit with widget active, tray,
  startup disabled by Windows, repeated cycles without orphan processes.
- Standard-user clean install, missing dependencies/Game Bar, offline use,
  upgrade while active, cancel/denied deployment, kill each update boundary,
  restart recovery, data/pin/placement preservation. Test public signing on a
  machine that has never trusted a development certificate.

## Dependency-ordered implementation

All steps are required for the complete release and end with reviewed evidence.

1. **Prove the platform together.** Minimal packaged Rust engine, Slint custom
   frame and widget pixels. Validate secured IPC, pin/click-through, exact DPI/
   placement, real fullscreen, independent lifetime, signing/dependencies and a
   two-version helper update. Measure engine/Game Bar baseline. Resolve failed
   platform/deployment premises before building the full interface.
2. **Data and complete renderer.** Typed documents, all static content/effects,
   validation/limits, shared pixels, caches, imports/exports and fixtures. Include
   hard text/SVG/blur cases here, not as postponed editor polish.
3. **Complete engine.** Keyboard/mouse capture and actions, profiles, persistence/
   recovery, authenticated IPC, tray, monitor handling and startup/shutdown. Prove
   real input delivery and switching latency before the UI depends on it.
4. **Complete Slint interface.** Shell, library, all designer controls, profiles,
   keybinds/settings, accessibility/dialogs/reconnect. Use shared renderer and
   transactions; compare pixels across preview and both outputs. Review against
   design/reference observations, not a generated starter template.
5. **Updates and release automation.** TDM UX, bounded download/verification,
   deployment recovery, production signing/package/version metadata and manual
   workflows. Exercise signed N-to-N+1 with all components active.
6. **Release qualification.** Full matrix, stress/resources, visual/accessibility
   review and independent adversarial review. Resolve required failures before
   calling it complete; publication needs separate authorization.

The first step has empirical gates, not undecided product scope. Slint native
frame integration, Game Bar pixel placement, package trust/dependencies, helper
lifetime and resource costs cannot be established from documentation alone.
The source tree contains the implementation; release readiness requires the
automated and native evidence above, including the signed update handoff.

[slint-renderers]: https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/
[slint-license]: https://slint.dev/agreements/slint-royalty-free-license.pdf
[tabler-license]: https://github.com/tabler/tabler-icons/blob/main/LICENSE
[directwrite]: https://learn.microsoft.com/en-us/windows/win32/directwrite/glyphs-and-glyph-runs
[gamebar-limits]: https://learn.microsoft.com/en-us/xbox/game-bar/known-issues
[gamebar-ipc]: https://learn.microsoft.com/en-us/xbox/game-bar/guide/communicating-apps
[msix-update]: https://learn.microsoft.com/en-us/windows/msix/non-store-developer-updates
[tdm-ui]: https://github.com/ohne-b/twitch-drops-miner/blob/736f6d20faccb6a194979928f0204a5ebec5e06c/frontend/src/features/settings/DesktopUpdates.tsx
[tdm-state]: https://github.com/ohne-b/twitch-drops-miner/blob/736f6d20faccb6a194979928f0204a5ebec5e06c/frontend/src/features/settings/UpdateCheck.tsx
[tdm-controls]: https://github.com/ohne-b/twitch-drops-miner/blob/736f6d20faccb6a194979928f0204a5ebec5e06c/frontend/src/shared/ui/index.tsx
[tdm-styles]: https://github.com/ohne-b/twitch-drops-miner/blob/736f6d20faccb6a194979928f0204a5ebec5e06c/frontend/src/styles.css
[tdm-verify]: https://github.com/ohne-b/twitch-drops-miner/blob/736f6d20faccb6a194979928f0204a5ebec5e06c/desktop/src/updates.rs
[tdm-release]: https://github.com/ohne-b/twitch-drops-miner/blob/736f6d20faccb6a194979928f0204a5ebec5e06c/.github/scripts/release.mjs
