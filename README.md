<p align="center">
  <img src="assets/logo.svg" alt="crosshair-rs placeholder logo" width="160" height="160">
</p>

<h1 align="center">crosshair-rs</h1>

<p align="center">A lightweight, customizable crosshair overlay for Windows.</p>

crosshair-rs aims to be a free, polished alternative to Crosshair X for Windows
10 and 11. A native Rust engine keeps the crosshair visible while a separate
dashboard opens only when you need to edit it.

**Status: repository groundwork.** There is no runnable application, installer or
release yet. The logo is a placeholder.

## Planned experience

- Pixel-perfect crosshairs with lines, a center dot, outlines, colors and presets.
- Instant preset switching through global keyboard bindings. Gameplay keys such
  as `E`, `1` and `2` continue reaching the game normally. Bindings intentionally
  work in every foreground application.
- A Tauri + React editor with immediate feedback and the same Rust renderer as
  the overlay. Closing the editor leaves the crosshair running.
- Local settings, a system tray menu and one Windows installer. No accounts,
  telemetry, background service or administrator requirement for normal use.

Near-zero idle CPU and low memory use are design targets, not measured claims.
The first milestone is a native overlay proof of concept, tested for click-through
behavior, physical-pixel placement, DPI and real-game compatibility.

## Compatibility

Desktop and windowed use are the initial targets; borderless-fullscreen games
need testing. Exclusive fullscreen is not guaranteed. Games and anti-cheat systems
may restrict overlays or input observation. This project will not inject into
games or bypass those restrictions.

## Development

Read [CONTRIBUTING.md](CONTRIBUTING.md) for the PR, validation and release workflow,
and [docs/architecture.md](docs/architecture.md) for the agreed technical baseline
and implementation order. Build commands will be added with the first runnable
milestone. Agents start with [AGENTS.md](AGENTS.md).

## License and credits

[PolyForm Noncommercial 1.0.0](LICENSE.md), copyright 2026 ohne-b (OhneB).
This project's own code and assets are source-available for noncommercial use,
with permission to modify and redistribute under the license's terms. Keep the
license and required notices with copies. Third-party components and the
Rust-derived logo retain their own licenses.

The placeholder [logo](assets/logo.svg) is adapted from the
[Rust Project's Rust logo artwork](https://github.com/rust-lang/rust-artwork/blob/main/logo/rust-logo-gear-only.svg).
The Rust and Cargo logos are owned by the **Rust Foundation** and licensed under
[Creative Commons Attribution 4.0 International (CC BY 4.0)](https://creativecommons.org/licenses/by/4.0/).
This modified version preserves the gear, mounting tabs and five screw holes,
and replaces the Rust logo's `R` with a centered crosshair, target ring and dot.
The [Rust trademark policy](https://rustfoundation.org/policy/rust-trademark-policy/)
also applies. crosshair-rs is not affiliated with or endorsed by the Rust Project
or the Rust Foundation.
