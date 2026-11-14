# Contributing

This guide applies to people and coding agents contributing to
[`ohne-b/crosshair-rs`](https://github.com/ohne-b/crosshair-rs), with `main` as the
canonical branch. Keep changes focused and discuss substantial features or
refactoring first unless already authorized. Favor simple, maintainable solutions
and measured performance. English only for now.

Contributor policy belongs here; [AGENTS.md](../AGENTS.md) is the agent entry point.
Technical contracts belong in [architecture.md](architecture.md).
Keep README short, user guides in `docs/` and release changes in CHANGELOG.
Preserve logo attribution and third-party licenses. Do not add contributor/PR
tables or README-rewriting bots.

## Workspace and privacy

Inspect `git status` and the diff before editing. Preserve unrelated work, local
settings, credentials, logs, profiles and backups. Do not reset, stash or rewrite
someone else's work. Use an isolated checkout when necessary; stage only intended
files. Never rewrite published history or tags without explicit authorization.

Before committing, inspect Git author and committer settings locally. For
maintainer work, verify the public `ohne-b` identity and noreply email against
GitHub account information or existing public commits; never guess the address.
Keep private names, emails, credentials and machine paths out of commits, artifacts
and review evidence. Never copy local CLI credentials into automation secrets.

Tests use temporary storage and synthetic profiles. Do not touch a running
installation, real settings or gameplay input. Manual input/overlay checks must
be deliberate and their limitations recorded.

Implementation, merge, release and deployment are separately authorized actions.
An ordinary code change never implicitly authorizes publication or deployment.

## Branches, commits and pull requests

Start a descriptive `feat/` or `fix/` branch from current canonical `main`. For an
origin pointing to this repository:

```powershell
git fetch origin
git switch -c feat/short-description origin/main
# Before final validation/review, and before merge if main advances:
git fetch origin
git merge origin/main
```

For forks, integrate canonical `upstream/main` and push to the fork. Resolve
conflicts deliberately and rerun affected checks. Use focused conventional
commits and PR titles, such as `fix(input): ignore held-key repeats`. Keep branch
names, commits and documentation free of assistant branding and conversation text.

Submit changes through PRs against `main`, never directly to it. The initial
empty-repository bootstrap needs a maintainer-authorized base commit before PRs
can be opened; it is not a continuing exception. Keep incomplete PRs in draft,
including those awaiting checks or independent review.

When merging is authorized, use a merge commit with a conventional subject and
the PR number: `fix(input): ignore held-key repeats (#123)`. Do not use GitHub's
default merge subject, squash or rebase instead of the merge commit.

## Development

Run commands from the repository root. Build both executables together:

```powershell
cargo build --locked -p crosshair-engine -p crosshair-editor
./target/debug/crosshair-editor.exe
```

For interactive testing and performance measurements, build both executables
with `--release` and run `target/release/crosshair-editor.exe`. The software UI
and preview renderers are substantially slower without compiler optimization;
an unoptimized debug build is not representative of interaction performance.

The editor starts its sibling engine. Closing the editor leaves the overlay
running; use the tray's **Quit** action to stop both. Game Bar and launch at
sign-in require an installed package. Unpackaged builds store their data in
`%LOCALAPPDATA%\crosshair-rs`.

## Validation

Use x64 Windows, the Rust toolchain in `rust-toolchain.toml`, Node 24, Visual
Studio 2022 C++ build tools and Windows SDK 10.0.26100. The Game Bar build restores
hash-pinned SDK/C++WinRT packages into ignored `target/`; it does not need the
Visual Studio XAML designer workload. Run the checks relevant to the change:

```powershell
git diff --check
npm --prefix scripts ci --ignore-scripts
npm --prefix scripts test
npm --prefix scripts run validate-workflows
./scripts/validate-sources.ps1
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
./scripts/build-gamebar.ps1 -Test
./scripts/package-windows.ps1 -OutputDirectory target/local-package
./scripts/test-installer.ps1 -ArtifactDirectory target/local-package
```

The [Validation workflow](../.github/workflows/validation.yml) runs this baseline on
Windows for PRs, main pushes and manual runs. Packaging builds the real engine,
Slint editor and Game Bar host, validates the MSIX manifest and bundles it with
the Microsoft-signed x64 VCLibs dependency in one NSIS setup EXE. Packaging itself
never installs the result. `test-deployment.ps1` installs and removes it only on
the disposable GitHub-hosted Windows runner. A PR package
is unsigned, has its updater disabled and cannot be promoted to a release.
`-Offline` on build/package scripts uses previously verified cached dependencies.
Choose a fresh package output directory rather than overwriting an artifact.
Packaging remaps local source/cache paths and checks the resulting executables;
`-SkipBuild` accepts only binaries already built with those privacy settings.

Packaging also runs `node scripts/generate-notices.mjs OUTPUT` against the locked
Windows normal/build dependency graph and bundles complete upstream license
material. Do not suppress missing-material failures. If a published crate omitted
its license, retain the exact original upstream text in a reviewed repository
file and add an exact `crate@version` entry in `scripts/license-overrides.json`:
`{"files":["repository/relative/license.txt"],"source":"https://upstream/version/LICENSE","reason":"Why this exact override is required"}`.
The collector rejects stale overrides, escaping paths and missing files; it
never invents copyright statements or emits local cache paths. Review the
selected license and redistribution conditions separately from file collection.

Inspect complete diffs, relative links and factual claims; run
`git diff --cached --check` for newly staged files. Automated checks do not prove
native click-through, DPI, real-game compatibility, Slint accessibility or package
installation. Record deliberate manual acceptance separately. Do not run overlays,
capture gameplay input or launch heavy builds during the user's active gameplay.

Run focused local checks during development. Successful CI on the final revision
supplies its full baseline; do not repeat the entire suite locally without reason.
New behavior and bug fixes need meaningful regression coverage, including failure
cases. Trivial documentation edits do not need application tests. After substantive
fixes or integration with main, rerun affected checks and update the evidence.
Record exact commands/results, CI links, skipped checks and limitations honestly.

## Independent review and evidence

Every PR, including documentation-only work, requires independent adversarial
review by a human or separate read-only reviewer who did not author it. An author's
reread does not count. Provide the goal, acceptance criteria, final revision/diff,
these instructions and actual validation evidence. Review correctness, privacy,
failure cases, regressions, scope, coverage and documentation claims.

Record the reviewer role, reviewed revision, findings and resolutions in the
[PR template](../.github/pull_request_template.md). Justify rejected findings with
evidence; have substantive fixes rechecked. Keep the PR in draft if review is
unavailable or blockers/checks remain unresolved. Agent review does not replace
required GitHub approvals. Record incorporated main and tested head SHAs and keep
the description aligned with the final change. Recheck current main and all
requirements before an authorized merge.

## Releases

Use the same deliberate PR/manual-release conventions as OhneGuessr and Twitch
Drops Miner. No ordinary merge, push or tag should publish a release.

The manual workflows implement these boundaries:

1. Prepare a version change on `feat/release-VERSION` through a draft PR.
   `[workspace.package].version` owns the product version; product
   crates inherit it and Cargo.lock agrees. Derive and validate widget/MSIX
   metadata using the version mapping in the architecture plan.
2. Add concise, reviewed notes to `CHANGELOG.md`, with relevant PR links in
   parentheses. Stable notes cover changes since the previous stable version.
   Use `vMAJOR.MINOR.PATCH` tags; prereleases use a SemVer suffix such as `-rc.1`.
3. Review and validate the final revision, then merge only when authorized.
   Publication needs separate explicit authorization and a manual workflow from
   validated canonical main. Require successful validation on the exact commit.
4. Create a draft release titled `crosshair-rs vVERSION`. Attach the validated
   setup EXE containing the unsigned Windows package, Game Bar widget and runtime,
   `latest.json` with updater signatures, and SHA-256 checksums. Verify
   downloaded assets and clean-install/upgrade behavior before publication.
   Never promote PR artifacts or silently rebuild a different revision.
5. Generate exactly one `Changelog:` comparison link and one issue-report link
   in the release body; do not duplicate those footers in changelog entries.
   Only stable releases advance latest. Never overwrite published versions/tags.

Keep credentials out of PR jobs. Configure scoped publication secrets through the
platform, never by copying a local login token. Updater signing is distinct from
Windows package signing; do not claim either until configured and verified.
Settings-driven checks/downloads and explicit Install and restart are part of the
complete plan; unattended installation is not. Follow the architecture's authenticated
version, artifact provenance, deployment and recovery contracts.

### Maintainer commands and signing configuration

Use **Prepare release** with a version such as `0.1.0-alpha.1`. Like TDM, it
updates Cargo.toml/Cargo.lock and opens a draft `feat/release-VERSION` PR. Add a
concise, reviewed CHANGELOG.md entry to that PR; notes omit the generated
comparison and issue footers. Its scoped repository `PUBLISHER_TOKEN` needs
contents and pull-request write permission so normal PR validation runs; do not
substitute a local login credential. `0.1.0-alpha.0` is the unreleased development
baseline; preparing alpha 1 must increase it, not publish the baseline.

Configure repository variables `CROSSHAIR_UPDATE_PUBLIC_KEY` (the project's
Minisign base64 public key) and `CROSSHAIR_PACKAGE_PUBLISHER` (the exact Windows
unsigned package publisher, `CN=ohne-b, OID.2.25.311729368913984317654407730594956997722=1`)
before validating a release candidate on main.
Both values are compiled into the engine and checked by packaging through
`crosshair-engine.exe --build-info`. They are public configuration, not secrets.
Changing either requires a newly validated artifact. Without them, validation
produces only an updater-disabled development package. Local release-candidate
builds explicitly use `./scripts/package-windows.ps1 -ReleaseCandidate` with both
variables set; no signing key is needed to build or validate that candidate.

The `prod` environment is restricted to `main` and supplies the complete,
unencrypted project-specific `MINISIGN_PRIVATE_KEY` file and `RELEASE_TOKEN`.
The latter must belong to `ohne-b`, with repository contents write and Actions
read permission. Provision and back up keys outside this repository. No Windows
code-signing certificate is used. The publisher identity belongs to Microsoft's
unsigned-package namespace and must remain stable between releases.

After release acceptance and explicit publication authorization, run **Publish
release** from main with only the version. It selects the latest Validation run
for exact current main and rejects old, failed, cancelled, superseded, expired,
dirty or PR artifacts. It copies the immutable setup EXE byte-for-byte and signs
both the EXE and its embedded MSIX, authenticating their version in `latest.json`,
then creates a draft release.
After downloading and verifying the assets it publishes that draft. Prereleases
do not advance Latest; stable releases do. A failed verification leaves the draft
unpublished for investigation; do not overwrite it or rerun blindly.

The draft contains only `crosshair-rs-VERSION-windows-x64-setup.exe`, `latest.json`
and `SHA256SUMS`. The workflow verifies all three assets, then verifies and extracts
the installer without elevation and authenticates the enclosed MSIX against the
original Validation hash. NSIS and Minisign remain build/release tools; users do
not download them separately.
Complete clean-machine installation, authenticated N-to-N+1 update, cancellation,
interrupted deployment and restart checks before publication. The updater uses
TDM's stable Latest endpoint by default. Optional prerelease discovery uses GitHub's
releases API and the selected release's `latest.json`, with the same signature
checks. Local development packages use `.Preview`
identity and have updates disabled.

For a release, download and run its setup EXE from the project's GitHub Releases
page. It checks embedded package identity and hashes, requests administrator
approval in the same Windows account, and calls the native Windows package
deployment API. Setup runs no PowerShell script or console window.
It requires x64 Windows 11 and never imports a certificate, enables Developer Mode
or installs a service. The EXE and MSIX are not Windows-signed. In-app updates
verify the project signatures before extraction and deployment.

For acceptance tooling, `/S /VERIFYONLY` checks the bundled files without
installation or elevation. `/S /EXTRACT` writes the MSIX beside the installer
only when that destination does not exist. Development setups bundle `.Preview`
packages with updates disabled; they cannot be promoted to a release. Alpha 1
requires a manual setup upgrade to consume the new updater metadata format.
