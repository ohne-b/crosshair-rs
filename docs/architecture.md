# Application design and implementation plan

> Proposed next feature: [Aim actions and simple animations](#proposed-aim-actions-and-simple-animations).
> That section is a plan, not a claim about implemented behavior.

This defines the application architecture and its complete acceptance contract.
Source implementation does not establish compatibility or performance: native
acceptance, installation and update qualification remain separate gates.
Contributor and release policy lives in [CONTRIBUTING.md](CONTRIBUTING.md).
The [design preview](design-preview.html) illustrates the proposed interface;
its interactions are illustrative, not an implementation of the application.

## Decisions and scope

| Area | Decision |
| --- | --- |
| Platform | Windows 11, x64. |
| Editor | Rust + compiled Slint, Winit backend, software renderer initially. No browser runtime. |
| Engine | Independent Rust process: state, input, tray, persistence, rendering, presentation and updates. |
| Desktop overlay | Compact, nonactivating, click-through Win32 layered window. |
| Fullscreen overlay | Pinned Xbox Game Bar widget, where Game Bar supports widget presentation. |
| Widget host | Thin C++/WinRT UWP XAML executable using Microsoft's Game Bar SDK. It displays engine-produced pixels. |
| Crosshair rendering | Shared Rust renderer: tiny-skia, Windows DirectWrite text outlines, bounded image/SVG decoding. |
| Communication | Local authenticated named pipes; versioned JSON control and bounded binary widget frames. |
| Storage | Engine-owned versioned JSON and managed local image assets. |
| Installation | One unsigned MSIX containing editor, engine and widget. Administrator approval for installation/updates; no Windows service. |
| Updates | TDM's Settings interaction and signed latest.json contract, adapted to MSIX. |
| Input | Global keyboard and mouse shortcuts for preset selection and visibility. No controllers. |
| Appearance | Orange placeholder logo, Tabler outline icons, Manrope interface font. |
| License | GPL-3.0-only with a Game Bar SDK linking exception; third-party components retain their licenses. |

The complete scope includes layered static editing, crosshair arms, primitive
shapes, images, freehand drawing, outlines, blur, local presets, manual
profiles, global visibility, keybindings, tray lifecycle, fullscreen integration,
import/export, updates, recovery, accessibility and packaging. The milestones
sequence this work; they do not make required features optional.

Exclude animations and animation stages, controllers, game detection, automatic
profile switching, foreground-process monitoring, cursor replacement, randomizer,
community feeds, workshop, shop, accounts, cloud sync, telemetry and streaming
servers. These are exclusions, not empty navigation
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

TDM reference revision: `f3f3b88e13d52108a5176b828965ac86ab457594`.
Sources: [settings updater][tdm-ui], [update state/provider][tdm-state],
[shared controls][tdm-controls], [styles][tdm-styles], [verification][tdm-verify]
and [release script][tdm-release]. Match its compact settings rhythm, help
popovers and update states; adapt its backend to the native architecture.

### Shell and styling

- Default 1280 x 800 logical pixels; minimum 960 x 640. Use a fixed 56 px left
  icon rail, with labels on hover/focus. On smaller available work areas, collapse
  the layer list and scroll the inspector. Never force a window beyond the work area.
- 48 px custom title bar: logo alone above the left rail; manual profile picker, active-preset
  thumbnail, a visibility power button with the configured chord in its tooltip,
  and custom minimize/maximize/close glyphs right. The Win32 frame adapter owns
  native caption hit testing/actions and Snap integration; native acceptance
  remains required. No On/Off text or profile
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
no runtime CDN requests. Use Slint's GPLv3 license and retain upstream notices
and the official Slint attribution badge. The project's Game Bar SDK linking
exception is limited to the separate widget host; it does not change Slint's
license. [Slint's terms][slint-license] describe the available choices.

## Screens and behavior

### Crosshairs and profiles

Saved and Browse tabs sit beside the Crosshairs heading and use bookmark and
search icons. Saved contains the local
thumbnail grid: search, All/Favorites with counts, a compact icon sort picker
for Manual/Name/Recently edited, New and Import. Names rename in place on Enter or
focus loss; Escape cancels. Favorite and trash appear on hover or keyboard focus;
favorites use a filled gray star, without changing the card border. Both star
states hide when neither hovered nor focused. Right-click/Shift+F10 opens the compact card menu:
Edit, Rename, Duplicate, Export, Create keybind, order and Delete. Menus and help
popovers anchor to their controls, dismiss outside/on Escape and return focus.
Browse contains built-in designs made from editable native layers, based on
the supplied references. Names are fixed; a bookmark appears on hover or keyboard
focus. Clicking a Browse card (or pressing Enter/Space) equips it for the engine
session without saving, changing visibility or opening Designer. The selection
survives dashboard closure; selecting a saved crosshair, using its hotkey or
switching profiles replaces it. Quitting the engine restores the saved selection
on next launch. The engine and dashboard share the built-in catalog in core;
temporary selection stays outside persisted state and undo history.
Save adds an independent copy to Saved without activating it or removing
the original. Saved copies use the existing edit, favorite and delete controls.
The engine validates and adds each copy atomically with fresh preset/layer IDs;
Undo removes the copy. There are no downloads or shared image assets.
Single-click activates; double-click/Edit opens Designer. Manual order supports
drag and keyboard alternatives. Move earlier/later switches the grid to Manual.
Cache thumbnails and virtualize larger lists. Keep the Designer's shape palette
separate from the Browse collection.

New creates a selected four-arm preset. Duplicate creates independent IDs while
sharing immutable assets. Delete offers Undo. If a preset has references, one
confirmation identifies affected bindings/profiles and the replacement; apply
atomically. Always retain one valid preset and profile.

A preset is a design. A profile groups shortcuts, selected preset, output scale,
offsets and saved positions. The preset library is shared: edits change that
design wherever selected; Duplicate makes an independent variant. Output mode
and monitor are app settings. The title-bar profile picker shows the active
profile and a Profile Settings link to Settings > Data, where Manage offers
Create/Rename/Duplicate/Import/Export/Delete. No game field.
Switch profile atomically, rearm held triggers after release and preserve global visibility.

The title switch, tray toggle and default remappable `Shift+Alt+Z` action operate
the same visibility state. The binding can be disabled. Passive observation does
not reserve OS/app shortcuts; explain collisions in keybinding help and never
modify Windows shortcuts automatically.

### Designer

Toolbar: editable document name, File, undo/redo, Save and Share. Save or
`Ctrl+S` commits the draft; ordinary edits never save automatically. The editor
updates its canvas and sends a validated transient preview to the engine without
changing the stored preset. File offers New, Open, Import, Duplicate and Export
crosshair. Share exports the current draft as PNG or SVG.

The layer list has a collapse chevron, ordered names, visibility and per-row trash.
Rename from the row; reorder by dragging, with neighboring rows moving aside. No footer menus
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
keys nudge one design pixel, Shift+arrow ten. Move deltas, resize size changes and
geometry fields use whole design pixels. Transforms preserve existing fractional
offsets and their center; rounding an untouched axis must not shift the design.
Resize measures the selection's actual span, not the handle's click offset.
Effects retain the renderer's discrete padding, so a blurred selection's measured
bounds can still change by more than one pixel at a padding threshold.
Exact fields cover every transform.

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

A compact color row contains swatch, hex and opacity; its anchored popup provides
HSV controls and a palette. Numeric fields support horizontal dragging, direct
entry, stepping and keyboard input without inspector sliders. Invalid/nonfinite
values keep the last valid result; intermediate text never clamps prematurely.
Zero-size components produce empty output predictably. The Image tool opens a
compact import picker; imported images use the existing bounded asset pipeline.

Designer undo/redo is local to the draft. A drag, color gesture, stroke or text
edit is one history entry; cancel restores that gesture's starting state. Target
100 entries within a 32 MiB budget, referencing immutable assets.
`Ctrl+Z`, `Ctrl+Y`, `Ctrl+Shift+Z`, clipboard shortcuts, `Ctrl+D` and bracket
ordering work. Text fields retain their own text shortcuts.

Pin the editing document ID. A global key may switch the overlay while typing,
but never redirect edits to another preset. Header shows the actually active
preset; a compact Use action appears when editing an inactive one. Closing or
opening another document with unsaved changes offers Save/Discard/Cancel.
Save waits for acknowledgement before continuing; failure keeps the draft.
Disconnect/reconnect preserves dirty drafts and never saves or overwrites them
automatically. A changed saved revision requires explicit conflict resolution.
Transient preview is session-owned and cleared on discard/disconnect; it never
enters durable state or engine edit history.

### Keybinds

One application table owns Toggle visibility; a separate full-width shortcut
table has Crosshair, Shortcut name and Keyboard / mouse columns plus trash.
Headers and body share column borders. The first row always contains only the
plus button and New; either opens the picker. Choosing a crosshair inserts a
separate row below New with its thumbnail, name, key recording and cancellation.
The completed shortcut stays in that row. Stopping recording leaves the pending
row available to retry; its X discards it. Keep the row until the saved binding
arrives, so asynchronous capture results cannot move another control under a click.
The thumbnail opens a reusable picker with search, All/Favorites counts and
named cards; clicking a card chooses immediately. Escape/outside dismisses and
returns focus. Shortcut names edit inline and default to the target preset name;
names never identify targets. Recording a disabled shortcut enables it.
Preset shortcuts select on press and belong to the active profile; the visibility
chord is application-wide. There are no press/hold or preset-toggle actions.
Legacy profile loading drops those retired bindings while preserving presets,
profiles and supported shortcuts; malformed or unknown actions still fail.

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
| Discord | On by default; General has an enable switch and Simple/Quote selector. Simple shares only overlay visibility; Quote uses only the Soldier: 76 quote. |
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

### Discord presence

The engine owns optional Discord Rich Presence through the local Windows IPC
pipe. The dashboard may close normally. Application ID `1570691376738209834`
and the large-image asset key `crosshair_rs` are public constants. The Download
button links to GitHub releases. No crosshair or
profile names, images, input, game detection, join data, uploads or secrets are
sent. The logo must be uploaded to that application's Rich Presence assets.

Reuse the existing bounded, cancellable Windows pipe transport and JSON support;
there is no additional dependency. One 256 KiB stack worker is created on first
enable. It sleeps on pipe input or settings notifications, uses a single latest
state slot, debounces changes for 350 ms and sends at most one activity update
per five seconds. Reconnection backs off from one to sixty seconds. Operations
have 500 ms deadlines and frames are capped at 64 KiB. Disable clears activity
and closes the pipe; the worker then sleeps until reenabled or shutdown. Exit
attempts a bounded clear and joins the worker. No Discord I/O runs on the input
or rendering thread. New and older settings without a Discord preference default
to enabled. An explicitly saved disabled preference is preserved.

### Stored model

Stable opaque IDs identify presets/layers/profiles/bindings/assets. Names are
editable labels, never identity or filesystem paths.

| Entity | Contents |
| --- | --- |
| App state | Schema version, settings, profiles/active ID, preset library, asset index, base selection/visibility, revisions. |
| Profile | ID/name, selected preset, bindings, scale/offset, saved positions. |
| Preset | ID/name, ordered layers, favorite, manual order, document revision and engine-owned global edit order for recency. |
| Layer | ID/name, visible/locked, transform, opacity, typed content/effects. |
| Content | Crosshair, rectangle, ellipse, triangle, text, image or freehand strokes. |
| Binding | ID, optional display name, chord, preset-selection or visibility action, target if needed, enabled. |
| Runtime only | Pressed devices/keys, capture, transient draft previews, connections, caches, edit history and update state. |

Geometry uses physical design pixels. UI logical pixels/display scale never
silently change thickness. Profile scale is an explicit transform; offsets are
physical output pixels. Values are finite/bounded; serialization does not depend
on enum memory layout or pointer size.

One engine owner serializes mutations. Commands carry target ID, expected
document revision and request ID. Acknowledgements publish revisions/events.
Reject stale edits with current state for resolution. Separate selection/settings
revisions from document revisions so a preset-switch key cannot invalidate an
unrelated edit.

The editor owns its unsaved draft and gesture history. Coalesce transient preview
updates once per display frame and send the final release. The engine validates
preview documents but never persists them or advances the saved document revision.
Only explicit Save submits an authoritative revision-checked replacement.
Lost connections retain dirty drafts for deliberate retry; never overwrite a
changed saved document or silently replay edits after reconnect.

### Imports and limits

Preset/profile/all-data exports use versioned self-contained JSON with embedded
assets. PNG exports transparent rendered output. SVG exports vector shapes and
text outlines, embeds cropped images at output resolution and raster outline
effects, and uses SVG Gaussian blur filters. Blur and image sampling may differ
slightly from the raster renderer. SVG output is capped at 16 MiB.
Both Share exports use the current draft without saving it. Imports validate the whole
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
Apply this alignment to rectangular contours after all layer and output transforms,
based on their final pixel spans. Preserve fractional spans and positions with
coverage antialiasing; do not infer alignment from scale factors alone.

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
- Select changes the base preset permanently; release never restores another
  preset. Visibility toggle changes the shared visibility state immediately.
- Profile change, removal, lock/suspend, lost input and capture cancellation
  block affected held triggers until release. Track releases without overrides;
  lost key-up must not leave stuck state.
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
Handshake verifies build/protocol before accepting commands. Control protocol v3
has capture tokens and independently ordered engine status. Connection epochs are
checked when queued events reach the editor, so a retired connection cannot change
the current UI. Both executables must be rebuilt together.

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

Debounce committed state by 300 ms, with a two-second maximum. Unsaved Designer
drafts and transient previews are excluded from snapshots. Write complete
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
Editor close fully exits its process; engine/presentation continue. A clean editor
can also close while disconnected. Editor crash does not stop the crosshair.
Only an explicit app launch, tray Open menu or an authorized update restart opens
the editor. Disconnects preserve drafts and offer an explicit reconnect; they do
not restart either process automatically.

Tray: Open menu, Show crosshair, compact active-profile preset submenu, Quit. Quit closes
editor, blanks/releases widget, unregisters input/tray, flushes and exits. No
orphan helper or respawn after deliberate Quit. An open editor offers Restart on
engine failure while preserving local edits; headless failures do not spawn a
surprise editor or a restart loop.

Windows draws the tray menus, including checkmarks, submenus, focus and keyboard
navigation. The engine opts into Common Controls v6 visual styles. A small Windows
11 theme adapter requests dark menus through optional `uxtheme.dll` exports;
missing exports or contrast themes retain system styling. It loads only from
System32, stays on the tray thread and adds no renderer, polling or dashboard
dependency. Menus release their native handles when dismissed, including on errors.

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
| Available | Version, Download, Release notes; reserve progress space. |
| Downloading | Thin progress, bytes, Cancel; percentage only with reliable total. |
| Verified | Ready-to-install status, Install and restart. |
| Installing | Disabled actions; state that overlay is closing for installation. |
| Check/download error | Short inline error with details behind i; check again or retry Download. |
| Install error | Recheck installed state, actionable recovery/manual release link; no blind retry loop. |

Use TDM's compact rhythm, 6 px bar and tabular values, no wizard/card stack.
Unknown totals show bytes only. Cancel removes partial data and returns Available.
A Settings dot may mark availability; never add another sidebar footer item.

### Manifest and authenticity

Endpoint: `https://github.com/ohne-b/crosshair-rs/releases/latest/download/latest.json`.
**Include prereleases** defaults off. When enabled, query the official GitHub
releases API, choose the highest supported SemVer with a `latest.json` asset, and
fetch that tag's metadata. Ignore drafts and non-application releases. Preserve
alpha/beta/rc suffixes in the interface and require the metadata version to match
the selected tag. Discovery uses no token, HTML scraping or background polling.
Bound API pages to 100 entries and 2 MiB, at most ten pages within the check's
20-second deadline; fail rather than choose from a truncated history.
Stable checks distinguish no published stable release from a broken release
asset using the API's latest-release endpoint. Changing the preference requires
idle update work and clears the previous offer after settings commit; checking
is blocked during that transaction. Never offer a downgrade.

TDM schema, with `windows-x86_64-nsis` platform and an embedded-package signature:

```json
{
  "schemaVersion": 1,
  "version": "1.2.3",
  "notes": "[Check release notes on GitHub](https://github.com/ohne-b/crosshair-rs/releases/tag/v1.2.3)",
  "pub_date": "2026-11-11T12:00:00Z",
  "platforms": {
    "windows-x86_64-nsis": {
      "url": "https://github.com/ohne-b/crosshair-rs/releases/download/v1.2.3/crosshair-rs-1.2.3-windows-x64-setup.exe",
      "signature": "BASE64_ENCODED_SETUP_SIGNATURE",
      "package_signature": "BASE64_ENCODED_MSIX_SIGNATURE"
    }
  }
}
```

Illustrative only, not a published release. Treat metadata as untrusted: small
size cap, strict schema/SemVer/HTTPS, exact repository/tag/asset validation and
bounded allowlisted GitHub asset redirects. Reject userinfo, unexpected ports,
query/path tricks. Notes are inert text/link, never HTML. Metadata timeout 20 s,
download deadline ten minutes, package cap 512 MiB enforced against actual bytes.

Stream the setup EXE to private staging, not a whole installer RAM buffer. Verify
the pinned project-specific Minisign key and authenticated trusted-comment
`version:VERSION` before executing its silent extraction mode. Keep the EXE locked
against replacement during extraction, bound that child to 60 seconds and cancel it
with the download. Independently verify the extracted MSIX using `package_signature`
and the same authenticated version, then match its identity/publisher/architecture.
The existing helper re-verifies that MSIX before shutdown and deployment.
Otherwise old signed bytes could be relabeled as a new version. A verifier that
requires contiguous input may use a bounded read-only mapping; do not claim
streaming cryptographic verification without its support.

Minisign authenticates the download; the Windows package itself is unsigned by
explicit product decision. Never install trust certificates or reuse another
project's private key. Rotate updater keys through a reviewed trust transition,
not a manifest-provided key. The implementation uses streaming prehashed Minisign
verification and native WinHTTP on a short-lived worker, with an absolute request
watchdog and cancellation. It has no idle networking thread. Signature, version,
identity and dependency failures block installation before the app stops.

### Install and restart

One NSIS setup EXE bundles the unsigned MSIX and its Microsoft framework. The
native installer checks pinned payload hashes (binding the build-validated identity)
before using Windows `PackageManager.AddPackageByUriAsync` with `AllowUnsigned`,
with no `.appinstaller` update association. It reuses a healthy installed x64
VCLibs runtime meeting the minimum version, supplying the bundled dependency only
when needed. This avoids redeploying a shared runtime used by other running apps.
Initial installation trusts the setup obtained from the project's GitHub release;
it does not claim Windows code signing or a self-authenticating executable.
[Microsoft's unsigned-package
deployment][unsigned-msix] requires Windows 11 and administrator privileges for
this executable package. Setup uses a temporary native helper elevated through
the normal UAC prompt; it verifies the same user, stages and rechecks payloads in
an administrator/SYSTEM-only directory, and reports Windows errors directly.
The existing in-app update coordinator still uses its separate PowerShell deployment
path. The engine, editor, widget and recovery coordinator stay unelevated. No Windows
App SDK dependency just for deployment.

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
5. Ask for administrator approval in the same Windows account. Copy the verified
   package into an administrator/SYSTEM-only directory, lock it against writes
   and replacement, and recheck its hash before deployment. Observe the result;
   cancellation preserves the existing installation. No downgrade flags. Query actually
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
Project updater-key verification is a release prerequisite. Test framework
dependencies on clean machines: C++ widget may need Microsoft VCLibs. Supply
trusted dependencies for offline distribution or state online acquisition
requirements. One application package does not imply zero runtime dependencies.
No separate Store-widget installation in the standard path. Game Bar itself is a
prerequisite only for its output mode. The package identity is
`OhneB.crosshair-rs`, with `Editor`, `Engine` and `Widget` app IDs and
`CrosshairOverlay` widget extension. Publisher is compiled as
`CN=ohne-b, OID.2.25.311729368913984317654407730594956997722=1`, Microsoft's
unsigned-package namespace. This does not claim certificate ownership. The three
executables are package-root siblings.

Taskbar logos use transparent target-size PNGs rendered from `assets/logo.svg`
at 16, 24, 32, 48 and 256 pixels, including unplated light/dark variants. Packaging
builds `resources.pri` with the Windows SDK so the shell resolves those variants.
Installer checks verify the index, packaged files and transparent backgrounds.

`scripts/package-windows.ps1` validates a real build and bundles an offline
`Microsoft.VCLibs.140.00` x64 **14.0.33519.0** dependency inside the setup EXE. The
framework is extracted from Microsoft's hash-pinned VS2022 CRT redistributable
CAB and checked for exact hash, identity and Microsoft signature. The embedded
deployment script passes it to Windows as a dependency without importing
certificates or changing Developer Mode. This is the UWP VCLibs framework,
not `UWPDesktop`. A hash-pinned NSIS toolchain builds the installer during Validation;
publication signs and copies those exact tested bytes without rebuilding.

Freeze the Microsoft framework minimum versions for the first stable release
line. Before stopping the app, validate that installed dependencies satisfy the
new package. A release requiring higher framework versions must extend the
signed update contract with pinned Microsoft-signed dependency assets, acquire
and verify them before handoff, and supply them to deployment; otherwise block
in-app installation with an actionable manual-install link while the old app
continues running. Never discover missing dependencies only after shutdown.

Workspace version owns product version; derive/check Rust, widget and MSIX
metadata. `A.B.C-alpha.N`, `-beta.N` and `-rc.N` map to fourth components `N`,
`20000+N` and `40000+N`, with `0 <= N < 20000`. Stable `A.B.C` maps to
`A.B.C.65535`; each version component fits 16 bits. This preserves SemVer order.
Release prereleases share the production identity; local development packages use
`OhneB.crosshair-rs.Preview` with updates disabled. Like TDM, the updater consumes
stable Latest by default, with optional prerelease discovery using versioned
`latest.json` assets. Prereleases never advance Latest.
`0.1.0-alpha.0` is the unreleased development baseline.

Follow existing OhneGuessr/TDM conventions:

- Draft release PR changes version/changelog; merge and publication are separate.
  No publish on ordinary pushes/tags.
- Validate exact canonical main and retain immutable artifacts with version,
  commit/platform/SHA-256 metadata. Publisher consumes that revision's successful
  artifacts, not an older success after a failed/cancelled newer attempt or a
  silent rebuild. Expired artifacts require revalidation.
- Manual publication signs both the validated setup and its unsigned MSIX with the
  authenticated release version. Both signatures live in latest.json; no verifier
  executable or detached signature is distributed. No signing secrets in PR jobs.
- Draft release gets only setup.exe, latest.json and SHA256SUMS.
  Download/verify assets and complete clean-install/upgrade/recovery evidence
  before publication. Only stable advances latest.
- Title `crosshair-rs vVERSION`, one Changelog comparison footer, one issue link.
  Never overwrite published tags/assets. Broken releases get a new corrected
  version; preserve old config until recovery succeeds.

`validation.yml` retains immutable `windows-SHA-RUN-ATTEMPT` artifacts for 14
days. `prepare-release.yml` creates the version draft PR; reviewed notes are added
to that PR. `publish-release.yml` signs the selected exact-main download, verifies
all downloaded assets and publishes the verified draft. Public build variables and protected
signing secrets, local commands and the final manual acceptance boundary are
specified in [CONTRIBUTING.md](CONTRIBUTING.md#maintainer-commands-and-signing-configuration).
Missing keys fail closed before any update request. Unsigned installation, helper
breakaway survival, two-version deployment and clean-install behavior require
actual runtime evidence; source checks alone do not prove those gates.
Validation installs the setup on its disposable hosted Windows runner, checks
registration and installed engine startup, then removes that test registration.
Interactive elevation/cancellation and upgrade recovery still need separate acceptance.

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
| Core | Bounds/references/revisions, history, transactions, profiles, input dispatch and release tracking. |
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
- Administrator-approved clean install, missing dependencies/Game Bar, offline use,
  upgrade while active, cancel/denied deployment, kill each update boundary,
  restart recovery, data/pin/placement preservation. Test on a machine that has
  never trusted a development certificate, including denial of elevation.

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
   deployment recovery, package/version metadata and manual
   workflows. Exercise authenticated N-to-N+1 with all components active.
6. **Release qualification.** Full matrix, stress/resources, visual/accessibility
   review and independent adversarial review. Resolve required failures before
   calling it complete; publication needs separate authorization.

The first step has empirical gates, not undecided product scope. Slint native
frame integration, Game Bar pixel placement, package trust/dependencies, helper
lifetime and resource costs cannot be established from documentation alone.
The source tree contains the implementation; release readiness requires the
automated and native evidence above, including the authenticated update handoff.

[slint-renderers]: https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/
[slint-license]: https://github.com/slint-ui/slint/blob/372cf0ee5577c3dfec309a45e7b778ba4e81b734/LICENSE.md
[tabler-license]: https://github.com/tabler/tabler-icons/blob/main/LICENSE
[directwrite]: https://learn.microsoft.com/en-us/windows/win32/directwrite/glyphs-and-glyph-runs
[gamebar-limits]: https://learn.microsoft.com/en-us/xbox/game-bar/known-issues
[gamebar-ipc]: https://learn.microsoft.com/en-us/xbox/game-bar/guide/communicating-apps
[msix-update]: https://learn.microsoft.com/en-us/windows/msix/non-store-developer-updates
[unsigned-msix]: https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package
[tdm-ui]: https://github.com/ohne-b/twitch-drops-miner/blob/f3f3b88e13d52108a5176b828965ac86ab457594/frontend/src/features/settings/DesktopUpdates.tsx
[tdm-state]: https://github.com/ohne-b/twitch-drops-miner/blob/f3f3b88e13d52108a5176b828965ac86ab457594/frontend/src/features/settings/UpdateCheck.tsx
[tdm-controls]: https://github.com/ohne-b/twitch-drops-miner/blob/f3f3b88e13d52108a5176b828965ac86ab457594/frontend/src/shared/ui/index.tsx
[tdm-styles]: https://github.com/ohne-b/twitch-drops-miner/blob/f3f3b88e13d52108a5176b828965ac86ab457594/frontend/src/styles.css
[tdm-verify]: https://github.com/ohne-b/twitch-drops-miner/blob/f3f3b88e13d52108a5176b828965ac86ab457594/desktop/src/updates.rs
[tdm-release]: https://github.com/ohne-b/twitch-drops-miner/blob/f3f3b88e13d52108a5176b828965ac86ab457594/.github/scripts/release.mjs


## Proposed Aim actions and simple animations

### Primary goal

Let users change crosshair visibility or temporarily use a saved crosshair while
aiming, and create simple per-layer responses to aiming or firing. Everything
continues working with the dashboard closed, with no additional animation work
for static designs.

Status: planning only. Keyboard and mouse first, as requested; no controller
column. This proposal changes the earlier static-only/no-press-hold baseline
only when implemented. It does not authorize a merge, deployment or release.
The behavior below is our proposed contract, not a claim of exact Crosshair X
compatibility.

### User interface and behavior

**Keybinds.** Expand the existing application table to four fixed rows:
Global toggle, Aim, Fire, Reload. Keep the separate Crosshair shortcuts table
and its empty New row. Use our existing dark surfaces, rounded outer table,
single separators, centered icons, orange selected accents and keyboard focus.
No controller placeholders, duplicate switches or explanatory paragraphs in rows.

| Row | Control | Initial key |
| --- | --- | --- |
| Global toggle | Existing enable switch and key recording | Shift+Alt+Z |
| Aim | Action dropdown; saved-crosshair thumbnail when relevant | Right mouse |
| Fire | Key recording; triggers the design's Fire animations | Left mouse |
| Reload | Key recording; resets the current design's animations | R |

Global toggle remains application-wide. Aim, Fire, Reload and crosshair shortcuts
belong to the active profile. Show the profile scope in the section label/help,
not in every row. Default keys must never replace existing assignments during
migration: leave a new conflicting role Unbound. Bound roles participate in
conflict checks even when the current design has no animation; runtime observation
can still be dormant until needed. The Aim action defaults to No action and
existing crosshairs remain static.

| Aim action label | Exact behavior |
| --- | --- |
| No action | No visibility or preset override; Aim animations may still respond. |
| Toggle crosshair (Press) | Each new press toggles the same master visibility state as Global toggle. |
| Hide crosshair (Hold) | Hide while held; release removes this visibility override. |
| Show crosshair (Hold) | With the master enabled, hide until Aim is held, then show while held. |
| Toggle saved crosshair (Press) | First press temporarily shows the chosen saved design; next press returns to the current base design. |
| Show saved crosshair (Hold) | Show the chosen saved design while held; otherwise show the current base design. |

For both saved-crosshair actions, choosing the dropdown entry immediately opens
`PresetPicker`, preselecting the previous target if valid. Choosing a card commits
(action, target ID) together. Escape/outside/Close cancels the whole pending change;
the old action and target remain. Never save a half-configured action. After
selection, show a square thumbnail beside the dropdown; clicking it reopens the
picker. Its tooltip/accessibility name includes the target name. Return focus
to the initiating control. Rename does not break the reference. Only Saved is
eligible: Browse designs must first be saved to become persistent action targets.
Use the current picker search, All/Favorites and empty states.

A normal saved shortcut, Browse selection or profile change clears any latched
Aim alternate. During a held alternate, a normal selection updates the base;
release reveals that new base, not the design selected before the hold. A base
can be a saved design or a session-only Browse design. These overrides never
save a preset, change favorites, rename documents or create undo entries.

The title-bar/tray switch controls master enabled state. Master Off always hides
the overlay, including Show actions, and clears active overrides/playback. The
switch reflects that master state; its tooltip can say why an enabled crosshair
is temporarily hidden, for example 'Hidden until Aim is held'. It must not flicker
with animation opacity. Enabling while a trigger is held waits for release and a
fresh press. Only an explicit Global toggle or Aim Toggle crosshair press can enable it
again; Fire, Reload, saved selection and animation playback cannot.

**Animate.** Add one compact rounded Design / Animate control in the Designer
inspector header. Use the same position, sizing and focus treatment as our other
segmented controls. No floating animation panel over the canvas and no second
copy of the mode switch. Design keeps the existing inspector unchanged.

Animate edits one optional animation on the selected layer: its normal design
is Base, and a second set of values is Animated. Show the supported properties
in the existing inspector, with a small reset action only on changed values.
Unchanged values inherit Base; changing Base later updates those inherited values.
An empty animation has no effect and no runtime work. An enabled switch permits
keeping a configured effect without playing it; Remove animation is undoable.
Switching Design/Animate itself neither dirties nor saves the document.

Start with these animatable properties:

- Every layer: X, Y, uniform scale, rotation and opacity.
- Crosshair layers: arm length, thickness, gap, center-dot diameter and the
  existing arm/dot fill colors. A disabled dot remains disabled; enable it in Design.
- Rectangle, ellipse and triangle: fill color, alongside the common transforms.
- Images, drawing and legacy text: common transforms/opacity only for now.

Reuse current field widgets and limits. Do not animate topology, arm count,
asset selection, crop, text content, blur or outline structure in this version.
Color alpha uses the existing rendering convention; opacity remains a separate
multiplier. Rotation follows the entered numeric delta (0 to 360 is a full turn).
Intermediate geometry may be fractional even when endpoint fields use integers.
Keep the renderer's existing pixel alignment and alpha coverage rules.

| Animation control | Choices / behavior |
| --- | --- |
| Trigger | Fire or Aim; Fire initially. |
| Activation | Hold or Press; Hold initially. |
| Duration | 150 ms initially; 1-10,000 ms per one-way movement. |
| Easing | Linear or Smooth; Smooth initially, with a fixed ease-in/out curve. |
| On release | Return, Reset or Pause; Return initially; shown only for Hold. |
| Repeat | Off or On; Off initially. |
| Direction | One way or Back and forth; One way initially. |
| Delay | 0-10,000 ms, initially 0; delay before a fresh activation, not every repeat. |

Keep Trigger, Activation and Duration visible. Put the remaining timing controls
in a collapsed Timing section; no timeline, stage list, curve editor or Add stage
button. Per-layer animations let an outline expand while a dot stays still.
Different layers can use different triggers; one layer has one trigger.

Playback rules are explicit:

- Hold starts on the physical down edge. With Repeat Off, One way reaches Animated
  and stays there until release. Back and forth reaches Animated, returns to Base
  and stays there until release. Duration applies to each leg.
- On release: Reset immediately restores Base; Pause freezes progress; Return
  smoothly blends the current values back to Base over Duration, starting from
  exactly the displayed values. If already at Base, it finishes immediately. A release during Delay returns
  to Base without a flash. Return never continues repeating.
- A new Hold resumes a paused effect. A fresh activation otherwise starts from
  Base; if pressed during Return, blend from the displayed values to Animated over
  Duration without another Delay. Resuming Pause also skips Delay.
- Press with Repeat Off plays one complete configured movement then resets to Base.
  Back and forth gives a smooth outward-and-back pulse; One way resets at its end.
  Repeated physical presses restart that effect, not queue more copies. Key repeat
  from holding a keyboard key does not count as a new press.
- Press with Repeat On toggles playback on/off on successive down edges; stopping
  restores Base. Hold with Repeat On repeats only while held. A One way loop resets
  at its boundary; Back and forth reverses there. The UI examples make that
  distinction visible rather than introducing extra playback modes.
- Reload stops and restores every animation on the effective crosshair, including
  paused effects. It changes neither the selected design nor visibility. Held
  triggers must be released before they can restart. Static crosshairs simply
  ignore Reload; it never sends a reload command to a game.
- A target selected by Aim is resolved before delivering that Aim activation, so
  its Aim animation can start immediately. Switching designs while Fire/Aim is
  held starts matching Hold effects on the new effective design; it never invents
  a Press event. Lifecycle resets below are the exception and require rearming.

Provide local preview buttons for Aim, Fire and Reset below the animation controls.
For Hold, pointer/key down starts and release ends; for Press, click/Space/Enter
sends one logical press. Preview uses the same evaluator as the engine but a
separate session, never synthetic OS input. Show Animated values for editing;
while playing, prevent accidental canvas transforms and provide an obvious Stop.
Stopping, leaving Animate or closing the document clears local playback. Playback
never changes the saved document or generates undo entries. Global hotkeys remain
global while the dashboard is focused, consistent with current behavior; local
preview does not claim to suppress physical mouse/keyboard input elsewhere.

Design thumbnails, picker thumbnails, PNG and SVG exports show Base. Native
crosshair documents, clipboard copies and profile/all-data exports retain the
animation definitions. No animated GIF/video export in this version.

### Phase 0 - Establish the existing contracts (discovery completed)

How this serves the primary goal: reuse working input capture, picker and output
paths instead of adding a competing implementation.

Read these locations before implementation; line numbers refer to the inspected
main revision `dd86caa56f546360787bd3de4651da30a8b06cff`:

- `crates/core/src/input.rs:11-118,132`: `InputEvent`, `InputOutcome`,
  `InputState::handle`, per-device keys, capture/reset/rearm. Outcomes already
  distinguish persistent changes from presentation changes.
- `crates/core/src/model.rs:584,748-904`: profiles, binding targets and the legacy
  deserializer which deliberately discards retired hold/toggle bindings.
- `crates/core/src/validation.rs:415-454`: target checks and
  `validate_binding_conflicts`; preserve subset modifiers and specificity rules.
- `apps/editor/ui/shared/preset-picker.slint:1-90`: `show`, `close`, `chosen`,
  `dismissed`, focus and filtering; existing callers in `ui/features/keybinds.slint`.
- `apps/editor/src/app/keybinds.rs:23-173`: tokenized capture, Replace/Cancel and
  `Command::SetBindings` atomic publication; `core/protocol.rs:95` defines the command.
- `apps/engine/src/app.rs:190-246`: `presentation_of` currently ignores the
  presentation preset ID and uses the base/Browse selection. Fix this shared
  selection resolution when adding temporary overrides, not just the input enum.
- `apps/engine/src/overlay/render.rs:17-135` and `runtime.rs:414-446,536-736`:
  render identity, cancellation, result processing, output and native timers.
- `crates/renderer/src/lib.rs:160,236`: `Renderer::measure` and
  `render_cancellable`; reuse the existing rasterizer for sampled layer values.
- `apps/editor/src/features/designer.rs:504-641` and `platform/preview.rs:16-119,220-350`:
  draft gestures, undo and asynchronous canvas rendering.
- `apps/engine/src/overlay/widget.rs:184-214`: `Widget::present`, newest generation;
  `apps/gamebar/CrosshairWidget/app.cpp:151-175` already receives the latest bitmap.

Allowed platform patterns: retain passive Raw Input and the engine's existing
`WM_TIMER` route. Editor-only preview may use
[Slint Timer::start/stop](https://docs.slint.dev/latest/docs/rust/slint/struct.Timer/),
on the Slint event-loop thread; it must not own real overlay playback.

Verification: independently trace these paths and the reference-maintenance callers
before edits. The current schema is 1, protocol is 4 and portable format is 1.
Do not revive retired bindings just because similar action names return.

### Phase 1 - Model, validation and migration

How this serves the primary goal: designs and key assignments survive saving,
sharing and upgrading without changing existing crosshairs.

Add one typed optional animation definition to Layer and one profile-owned action
configuration holding Aim/Fire/Reload chords and the Aim action/optional target.
Reuse InputChord, IDs, field validation and Command::SetBindings rather than
string-based property expressions or separate settings stores. Keep playback
progress, pressed keys, paused values and alternate selection outside AppState.

Use typed optional target values, limited to the supported properties above;
no arbitrary JSON paths, scripts or duplicated full layers. Validate finite
numbers, legal enums, duration/delay bounds, target existence, supported property
for the layer type and no unknown fields. Preserve existing document/asset bounds.
One disabled animation per layer is enough; no unbounded track list.

Introduce schema 2, protocol 5 and portable format 2 with explicit old-format
loaders. Migrate v1 into defaults before strict validation; keep the original and
backup until atomic persistence succeeds. Retired v1 bindings remain retired.
Old application versions must refuse new formats rather than silently strip data.
Accept both native/portable versions for import; write the new version. Preserve
existing static content and asset IDs byte-for-byte where representation permits.

Cover every reference operation: preset deletion/replacement, duplication,
profile copy, export inclusion, import ID remapping and cache warming. Reuse
`BindingAction::target`-style centralized target enumeration for the new Aim target;
update `commands.rs:184-213`, `storage/portable.rs:157-170,434`,
`features/library.rs:43` and `runtime.rs:675`. Deleting an Aim target uses the
existing affected-reference confirmation and atomic remapping, never a dangling ID.

Verification: v1 migration fixtures including retired bindings/conflicting default
keys; v2 native/profile/all-data/clipboard round trips; duplicate IDs; target
replacement; malformed/nonfinite values; unsupported versions fail without writes.
No new package, dependency or general animation framework is needed.

### Phase 2 - Action input and crosshair selection

How this serves the primary goal: Aim behaves predictably during actual play,
including quick switching, interruptions and cancelled configuration changes.

Extend InputState with Aim/Fire down/up and Reload edges, keeping the existing
per-device state, modifier specificity and capture token model. Required modifier
release terminates a held chord; a second device holding the same input prevents
premature release. Aim and Fire always require release-capable keyboard/mouse
inputs, even when the current effect uses Press. Wheel/Pause pulses remain available
for ordinary press shortcuts and Reload. This keeps shared designs compatible with
all profiles without a Hold animation invalidating another profile's input. Reload
is always a press.

Aim action plus Aim animation are two effects of the same resolved logical event.
An independent Fire, Reload, preset shortcut or global toggle cannot also win that
physical event. Use the existing conflict evaluator and atomic Replace/Cancel;
include new roles across active-profile/global assignments, while preserving the
existing more-specific chord rule. Validate the release-capable Aim/Fire restriction
on binding changes, every imported profile and migration. Validate shared animation
definitions independently of the currently selected profile.

Create one effective-selection/visibility resolution used by status, rendering,
editor active indicators and tray. Layer Aim overrides over the latest saved or
Browse base; never restore a stale snapshot of all AppState. Clear latched/held
runtime state on profile change, binding edit, capture entry, lock/suspend,
device loss, output reconfiguration, import/reset and shutdown. Do not clear
normal held state merely because the dashboard closes. Existing commit/reset
paths must be audited explicitly, as `commands.rs:367-383` resets input on commits.

Observe releases during capture/frozen operations; block affected held inputs
until release after a lifecycle reset. Keep global input passive and global:
no game detection, hooks, suppression, injection or keyboard/mouse polling.
Register mouse Raw Input only when a relevant action/animation, capture or pending
release needs it. Motion stays discarded without allocations.

Wire the four table rows and pending picker transaction to existing Slint controls.
Do not alter the separate New shortcut flow. Handle late picker/capture/save replies
using tokens/revisions so they cannot commit into a different profile or action.

Verification: each of the six Aim actions, master Off, repeat keys, two devices,
modifier release, Browse restoration, base change during hold, profile changes,
target deletion, capture cancel/disconnect/expiry, lock/unlock and lost key-up.
Include Cancel/Replace and picker Cancel tests that assert no persistent changes.

### Phase 3 - Deterministic animation evaluation and overlay playback

How this serves the primary goal: the overlay responds smoothly without keeping
the dashboard open or making static crosshairs more expensive.

Put a small pure evaluator in core, using monotonic elapsed time passed by the
caller. It returns sampled ordinary layer values and whether another frame is
needed. No renderer/window/clock dependencies in core, no persistent mutation.
Implement the trigger, pause, return, repeat and reload rules above with a fake
clock in tests. Different layer animations remain independent and deterministic.

The engine owns actual playback. Use the current render worker and output paths,
with a distinct runtime generation/sample identity separate from saved revisions.
Keep static render keys/cache unchanged. Animated frames bypass the static LRU;
retain only bounded in-flight/latest presentation buffers. Never precompute a
filmstrip or duplicate decoded images per frame.

A frame must not call full Runtime::refresh, publish AppState, write storage,
update Discord or rebuild tray menus. Add a presentation-only route using the
existing Desktop/Game Bar output functions. Send semantic status changes only
when selection/visibility/action state changes; bitmap frames stay on the widget
channel. C++ displays frames and receives no animation engine.

Use at most one in-flight render and one newest desired sample. Timer ticks must
not cancel a still-valid render repeatedly: that would starve expensive designs.
When it finishes, present a result valid for the current design/output generation
and sample the current time for the next job. Selection, reset or output changes
invalidate obsolete generations immediately. Drop overdue frames instead of
queueing them or slowing logical animation time.

Target at most 60 presentation frames/sec while values are changing. Use elapsed
time, never frame counts, and no global high-resolution timer request. Stop the
animation timer for static/paused/settled states. Hidden/disconnected/unpinned
outputs do not rasterize; keep logical timing from timestamps and sample the
current state if they become visible again. Suspend/lock instead clears/rearms
playback. Complex effects may achieve less than 60 fps; measure rather than promise.

Validate sampled frames as well as endpoints: rotation/combined transforms can
exceed raster limits between valid endpoints. Preserve the last valid frame and
stop the affected effect with one actionable error; do not flood status or retry
forever. Keep the existing 1024px raster and other resource bounds. Use layer
anchors consistently; changing sampled bounds must not make the crosshair drift.

Verification: fake-clock boundary/zero-change/repeat/pause/return/retrigger tests;
mid-transition bounds failures; stale render completion after reset; no-starvation
slow-render test; alpha/rotation/centering goldens on both output paths. Confirm
no animation frame changes document revisions, undo, persistence or Discord state.

### Phase 4 - Designer controls and local preview

How this serves the primary goal: users can create the intended effect in one
place and see it without learning an animation authoring tool.

Reuse `features/designer.rs::fields`, numeric/color controls and draft
begin_gesture/finish_gesture for definition edits. Animation target editing creates
one undo step per gesture; Reset property/Remove animation are ordinary undoable
edits. Multi-selection shows only compatible common controls, or asks the user
to select one layer for animation configuration; initial implementation uses the
latter to keep the UI unambiguous.

Reuse the asynchronous Preview worker, with a playback path that does not rebuild
all hit-test geometry, layer thumbnails and inspector models every frame. The
same core evaluator samples the document; a local timer exists only during a
running local preview. A dirty definition uses the existing transient-draft
protocol, sent when edited, not as a stream of evaluated document snapshots.
Editing an inactive document stays pinned to that document even while global
keys switch the actual overlay.

Use current Theme tokens and Tabler icons. Match field heights, align dropdowns,
keep normal spacing between actions, and anchor tooltips to controls. Keep animated
properties readable without showing floating-point noise. Labels, selected state,
focus outlines and per-property reset must be usable by keyboard. Respect reduced
motion for UI transitions; preview playback is explicit, and designed overlay
animations remain an intentional user feature rather than decorative UI motion.

Verification: save/discard/conflict/reconnect while editing animation; undo/redo;
keyboard-only configure/picker/preview; no stuck Hold on pointer cancellation;
Design/Animate toggling without dirtying; static export versus native-definition
round-trip; small window and 100/125/150/200% scaling. Native visual checks required.

### Phase 5 - Acceptance, review and delivery

How this serves the primary goal: ship a complete feature whose controls, saved
state and real overlays agree, with measured overhead.

Build optimized engine/editor together and compare the same representative designs
against the current static release. Measure idle CPU/private memory with dashboard
open/closed, first-trigger delay, frame-time distribution and sustained playback
for simple and maximum-practical designs. Repeat on Desktop and Game Bar, and under
8 kHz mouse motion. Include simultaneous Aim+Fire, rapid preset switching and
repeated dashboard open/close. Check memory stabilizes after repeated use; investigate
regressions before expanding scope. Active animation uses CPU; no RAM/CPU guarantee
is made before these measurements. Static operation must add no animation timer,
render job or per-frame transport traffic.

Run relevant core/renderer/engine/editor tests, Rust formatting/Clippy, source and
workflow validation and the normal Windows package CI. Validate both successful
upgrade and refusal by an older app using synthetic data, plus portable imports.
Use deliberate native acceptance for focus/click-through, fullscreen, key capture,
input cleanup and install/upgrade; do not call these passed from headless tests.

Use separate implementation commits for model/migration, action input and UI,
engine animation playback, Designer controls, and final regression/docs work.
Keep the feature PR in draft until the whole slice works; do not expose dead Fire
or Reload controls in a partial release. Obtain an independent read-only review of
the final revision and actual evidence, then provide a local test build
when implementation is authorized. Merge/release remain separate actions.

Update the existing architecture sections to the implemented contract, user guides
with short Aim/animation examples, and CHANGELOG. Keep README a short introduction.
Document that these are input-driven visual effects, not measurements of weapon
spread/recoil or game state. Defaults add no movement to existing designs.

Not in this version: controllers, autoplay, multiple stages/keyframe timelines,
custom easing curves, arbitrary animated properties, animated image/video exports,
game detection or game-specific recoil/ammo tracking. These omissions keep the
requested Aim/Fire/Reload behavior complete without adding another editor inside
our editor.


Plan review: a separate read-only reviewer checked the input, UI, persistence and
rendering contracts. The review clarified exact Return/retrigger timing, required
release-capable Aim/Fire inputs across profiles, and the explicit visibility-toggle
exception to master Off. These clarifications are incorporated above. Source
inspection and document whitespace validation were performed; implementation,
performance measurement and native acceptance have not been performed.
