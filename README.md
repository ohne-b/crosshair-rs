<p align="center">
  <img src="assets/logo.svg" alt="crosshair-rs placeholder logo" width="256" height="256">
</p>

<h1 align="center">crosshair-rs</h1>

<p align="center">A lightweight, customizable crosshair overlay for Windows.</p>

<p align="center">
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-PolyForm_Noncommercial_1.0.0-F97316" alt="License: PolyForm Noncommercial 1.0.0"></a>
</p>

crosshair-rs aims to be a free, polished alternative to Crosshair X for Windows
10 and 11. A native Rust engine keeps the crosshair visible while a separate
dashboard opens only when you need to edit it.

**Status: repository groundwork.** There is no runnable application, installer or
release yet. The logo is a placeholder.

## Planned experience

- Layered static crosshairs with shapes, text, images, freehand drawing, outlines
  and blur, using one shared renderer for the editor and overlay.
- Instant preset switching through global keyboard and mouse bindings. Keys such
  as `E`, `1` and `2` continue reaching the game normally. Bindings intentionally
  work in every foreground application.
- A native Slint editor with immediate feedback, manual profiles and a custom
  title bar. Closing the editor leaves the crosshair running.
- Local settings, a system tray menu, one signed Windows package and updates
  through Settings. No accounts,
  telemetry, background service or administrator requirement for normal use.

Near-zero idle CPU and low memory use are design targets, not measured claims.
The first milestone proves desktop and Game Bar presentation, click-through,
physical-pixel placement, DPI, real-game compatibility and package/update lifetime.

## Compatibility

The plan targets desktop, windowed and borderless use through a native overlay,
plus fullscreen applications supported by Xbox Game Bar through a small widget.
This does not guarantee every exclusive-fullscreen game: Microsoft documents
[Game Bar limitations](https://learn.microsoft.com/en-us/xbox/game-bar/known-issues),
including Vulkan/OpenGL fullscreen cases. Actual widget visibility, placement
and input require testing. Games and anti-cheat systems may restrict overlays or
input observation. This project will not inject into games or bypass restrictions.

## Development

Read [CONTRIBUTING.md](CONTRIBUTING.md) for the PR, validation and release workflow,
and [docs/architecture.md](docs/architecture.md) for the complete application plan,
screen behavior, technical contracts and acceptance tests. The
[design preview](docs/design-preview.html) illustrates the proposed interface.
Build commands will be added with the first runnable milestone. Agents start
with [AGENTS.md](AGENTS.md).

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
This modified version preserves the gear, mounting tabs and five screw holes in
charcoal (`#3F454D`), and replaces the Rust logo's `R` with four slim crosshair
bars in orange (`#F97316`).
The [Rust trademark policy](https://rustfoundation.org/policy/rust-trademark-policy/)
also applies. crosshair-rs is not affiliated with or endorsed by the Rust Project
or the Rust Foundation.
