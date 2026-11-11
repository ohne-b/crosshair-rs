# Architecture and test ownership

This is the agreed implementation baseline, not a claim that these components
already exist. Build in the order below; reconsider decisions when measurements
or compatibility tests provide a concrete reason. Keep technical contracts here
and contributor/release policy in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Product and boundaries

Target Windows 10/11 first. Prioritize precise rendering, instant preset switching,
responsive editing, reliability and minimal idle CPU/RAM. Measure actual resource
use with the editor closed. No accounts, telemetry, Windows Service, database,
HTTP IPC server or administrator privileges for normal use.

Use one Cargo workspace and two independent executables:

```text
Cargo.toml
apps/
  engine/src/          # Native Win32 overlay, input, tray, IPC and persistence
  dashboard/
    src/               # React + TypeScript + Vite
    src-tauri/         # Tauri v2; thin Rust IPC adapter and preview rendering
crates/
  core/                # Shared types, profiles, validation and protocol messages
  renderer/            # Deterministic Rust rasterizer shared by both executables
```

Create directories/modules when needed, not as empty scaffolding. The engine may
use `overlay.rs`, `input.rs`, `keybinds.rs`, `tray.rs`, `ipc.rs` and `settings.rs`;
avoid more crates without demonstrated reuse or complexity. All native logic is
Rust. Initial dependencies are the `windows` crate for Win32, `tiny-skia` for
rasterization and `serde` for small versioned JSON contracts; pin actual versions
when the first implementation requires them.

The engine owns profiles, settings, persistence, overlay, global keyboard input,
tray and IPC. The dashboard presents and edits engine state through its Tauri
backend; it never writes authoritative settings or controls the overlay directly.
The renderer is shared by the real overlay and Rust-backed dashboard previews.
Do not duplicate crosshair geometry in CSS or SVG.

## Rendering

Start with a compact native transparent layered window around the crosshair.
Use `WS_EX_LAYERED`, `WS_EX_TRANSPARENT`, `WS_EX_TOPMOST`, `WS_EX_TOOLWINDOW` and
`WS_EX_NOACTIVATE`; verify click-through and focus behavior on real Windows.
Use `UpdateLayeredWindow` with premultiplied bitmap data. Convert tiny-skia's
premultiplied RGBA to Win32 BGRA without premultiplying twice.

Render on settings/display changes, with cached images for preset switching.
Static crosshairs have no continuous render loop. Support physical-pixel center
placement, offsets and monitor selection. Establish Per-Monitor DPI Awareness V2
before creating windows; handle scaling, resolution changes and monitor changes.
Account for odd/even thickness and center alignment to avoid blur. A GPU renderer
is only worth reconsidering if measured needs such as animation justify it.

The shared model should cover lines/dimensions, gap, thickness, color, opacity,
outlines, center dot, position and saved presets. Circles/additional primitives
can follow as justified. Validate ranges and bound bitmap allocations. Do not
assume anything about Crosshair X's internal representation.

Renderer tests own deterministic geometry, alpha/channel conversion, outlines,
odd/even alignment and representative DPI/resolution cases. Native manual checks
own actual focus, click-through, placement and display-change behavior.

## Global keybindings

Use Raw Input: `RegisterRawInputDevices` with `RIDEV_INPUTSINK`, receiving
`WM_INPUT` / `RAWKEYBOARD` through the engine's native message loop. Switch on
key-down; track pressed keys so held-key repeats do not trigger more switches.
Handle key-up, modifiers, extended key codes and state recovery deliberately.

Ordinary keys such as `E`, `1` and `2` must still reach the foreground application.
Never suppress, inject or simulate input. Do not use constant `GetAsyncKeyState`
polling or reserve gameplay keys with `RegisterHotKey`.

Bindings always work globally. Pressing `E` in Discord also switching presets is
intentional. No game detection, foreground-process monitoring, game restrictions
or automatic game-profile switching. Multiple keys may select the same preset.
Keyboard input comes first; mouse/controller support is undecided.

Test press/release/repeat transitions, modifiers, invalid bindings and rapid preset
switches. Verify global delivery and noninterference manually, including sleep/wake
and keyboard reconnection. Synthetic state tests cannot prove real-game behavior.

## IPC and persistence

Use Windows named pipes with small serde JSON request/response messages. Restrict
access to the current user and session using OS security, not just a pipe name;
reject remote clients. Bound message sizes, validate requests and version the
protocol so mismatched executables fail clearly. Handle disconnects, reconnects
and engine startup without competing writers or unbounded retries.

Initial command candidates are `GetState`, `UpdateCrosshair`, `SelectPreset`,
`ToggleVisibility`, `GetPresets`, `UpdateKeybinds` and `Shutdown`. Add only what
the current milestone needs. Send updates on state changes, not polling timers.

The engine owns local versioned JSON configuration, atomic saves, appropriate
debouncing, future schema migrations and JSON profile import/export. Restore the
selected preset and relevant settings at startup. Preserve existing data on
malformed input, write failure or unknown future versions; never silently replace
it with defaults. Flush pending changes during graceful shutdown.

Core tests own validation, protocol/schema versions and state transitions.
Engine tests own IPC framing/access/disconnects and temporary-directory persistence
round trips, migrations, corruption, write failures and restart recovery.

## Process lifecycle

Opening the app starts or connects to one engine per user session. Opening the
editor again reuses/focuses its existing instance. Closing the editor fully exits
its process and WebView while the engine stays alive.

The engine owns the tray: Open editor, Toggle crosshair and Quit application;
preset selection is optional. Quit stops the engine and closes an open editor.
Optional launch-at-login starts only the engine. Coordinate shutdown/restart
during installation or updates so both executables remain compatible.

Package both executables in one Windows installer using Tauri sidecar/external
binary support. Prove that launching via the chosen mechanism preserves the
independent engine lifetime; do not assume sidecar lifetime semantics.

Test simultaneous launches, editor crashes, engine restarts, tray reopening,
graceful quit, sleep/wake and version mismatch. Installer tests must cover clean
install, upgrade with the engine running and preservation of user configuration.

## Dashboard

Use Tauri v2 + React + TypeScript + Vite only after validating the native core.
Provide immediate live preview with no normal Apply step. Support preset creation,
rename, duplicate, selection and deletion, intuitive bindings and sensible defaults.
Keep keyboard accessibility, focus, readable labels and responsive layouts intact.
UI design is not finalized; avoid inventing settings or features to fill a screen.

Frontend tests own editing flows, reconnect/error handling, accessible controls
and stale-preview prevention. Integration tests establish that preview and overlay
use the same renderer and that settings remain engine-authoritative.

## Compatibility and implementation order

External Win32 overlays target desktop/windowed use first. Test borderless games;
exclusive fullscreen is not guaranteed. Games and anti-cheat systems may restrict
overlays/input observation. Never inject code or bypass restrictions. Record tested
Windows versions, display setups and games rather than claiming universal support.

1. Native Win32 overlay proof of concept.
2. Validate transparency, click-through, positioning, DPI, performance and games.
3. Shared crosshair model and tiny-skia renderer.
4. Preset cache and global Raw Input switching.
5. Engine lifecycle, tray, local persistence and named-pipe IPC.
6. Tauri/React dashboard and shared-renderer previews.
7. One installer for both executables and manual release automation.
8. Broader recovery/compatibility QA, measurements and UI polish. Add meaningful
   tests during each preceding milestone rather than postponing all tests here.
9. Consider optional features only after the core is reliable.

Discord presence, mouse/controller bindings, animation, advanced shapes, SVG
import/export, automatic update UX and advanced tray features remain undecided.
Do not let them complicate the initial core.
