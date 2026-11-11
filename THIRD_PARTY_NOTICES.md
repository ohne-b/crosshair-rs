# Third-party notices

crosshair-rs's own code is licensed under
[PolyForm Noncommercial 1.0.0](LICENSE.md). That license does not replace the
licenses of the components and artwork below. Preserve their notices when
redistributing the application.

## Rust-derived application logo

The placeholder [application logo](assets/logo.svg) adapts the
[Rust Project logo artwork](https://github.com/rust-lang/rust-artwork/blob/main/logo/rust-logo-gear-only.svg).
The Rust and Cargo logos belong to the **Rust Foundation** and are licensed under
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/).
This modified version uses a charcoal gear, mounting tabs and five screw holes
and replaces the original `R` with four orange (`#F97316`) crosshair bars.
The [Rust trademark policy](https://rustfoundation.org/policy/rust-trademark-policy/)
also applies. The project is not affiliated with or endorsed by the Rust Project
or Rust Foundation.

## Fonts and icons

- **Manrope:** Copyright 2018 The Manrope Project Authors. Distributed under the
  SIL Open Font License 1.1. The full supplied notice is
  [assets/fonts/OFL.txt](assets/fonts/OFL.txt), also included in the installed
  package as `Licenses/Manrope-OFL.txt`.
  [Upstream project](https://github.com/googlefonts/manrope).
- **Tabler Icons:** Copyright (c) 2020–2026 Paweł Kuna. Distributed under the MIT
  License. The full supplied notice is
  [assets/icons/LICENSE](assets/icons/LICENSE), also included as
  `Licenses/Tabler-MIT.txt`.
  [Upstream project](https://github.com/tabler/tabler-icons).

## Slint and Rust dependencies

The Windows editor uses Slint **1.18.1** under its
[Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0](https://github.com/slint-ui/slint/blob/372cf0ee5577c3dfec309a45e7b778ba4e81b734/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md).
This is the selected license for Slint's integration into this desktop
application. Slint's other offered license texts remain intact in the bundled
upstream notices. Its required public attribution badge is displayed in the
repository README. Slint remains subject to its attribution, application-use
and redistribution conditions; the project license does not relicense it.

Every package contains `Licenses/third-party-rust.md`, generated from the exact
locked Windows dependency graph reachable from the engine and editor. It lists
crate versions, declared license expressions, repositories/source links and the
full supplied license files, including build dependencies. Development-only and
unreachable platform dependencies are excluded. The file preserves upstream
copyright statements rather than inventing generic authorship. Source packages
are linked by exact version. Packaging fails when required material is missing;
reviewed overrides must identify an exact crate version and original source.

## Microsoft components

The Game Bar host uses **Microsoft.Gaming.XboxGameBar 7.3.2607010**. The package
includes Microsoft's unchanged SDK runtime files and its complete supplied
`Microsoft.GameBar.LICENSE.txt`. These remain under the
[Microsoft Game Bar SDK terms supplied with that NuGet version](https://www.nuget.org/packages/Microsoft.Gaming.XboxGameBar/7.3.2607010),
not PolyForm. The SDK terms cover distributable code and its distribution
requirements, restrictions, Microsoft services and data collection. Xbox Game
Bar and Windows themselves are separate Microsoft products.

The offline installer includes the unchanged, Microsoft-signed
**Microsoft.VCLibs.140.00**, x64 **14.0.33519.0**, framework. It is acquired from
the Microsoft Visual Studio 2022 CRT Appx redistributable **14.44.35208** with
pinned package hashes. This framework and the Microsoft C++ runtime portions of
the native host remain subject to the applicable Microsoft Visual Studio license
terms. Microsoft's
[Visual Studio 2022 redistribution list](https://learn.microsoft.com/en-us/visualstudio/releases/2022/redistribution#universal-windows-apps-and-windows-store-apps)
identifies the retail VCLibs Appx as an unmodified redistributable for sideloaded
Universal Windows applications. Redistribution rights depend on the distributor's
validly licensed development tools and the applicable terms; this notice grants
no additional rights over Microsoft components.

The C++/WinRT projection tool **2.0.250303.1** is a build dependency under the
[C++/WinRT MIT License supplied with that NuGet version](https://www.nuget.org/packages/Microsoft.Windows.CppWinRT/2.0.250303.1).
Its supplied license is included as `Licenses/CppWinRT-LICENSE` when packaging.
Windows API use does not imply Microsoft endorsement of crosshair-rs.

## Maintenance tools

Node maintenance dependencies are pinned in `scripts/package-lock.json`; their
own package licenses apply. The release runner uses the upstream
[Minisign 0.12 tool](https://github.com/jedisct1/minisign/tree/0.12), under its ISC
License, to sign artifacts. Maintenance tools and private signing keys are not
bundled into the installed application.
