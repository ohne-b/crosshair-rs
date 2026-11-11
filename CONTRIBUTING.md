# Contributing

This guide applies to people and coding agents contributing to
[`ohne-b/crosshair-rs`](https://github.com/ohne-b/crosshair-rs), with `main` as the
canonical branch. Keep changes focused and discuss substantial features or
refactoring first unless already authorized. Favor simple, maintainable solutions
and measured performance. English only for now.

Contributor policy belongs here; `AGENTS.md` is the short agent entry point.
Technical contracts belong in [docs/architecture.md](docs/architecture.md).
README describes the product and its actual status. Preserve logo attribution and
third-party licenses. Do not add contributor/PR tables or README-rewriting bots.

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
```

The [Validation workflow](.github/workflows/validation.yml) runs this baseline on
Windows for PRs, main pushes and manual runs. Packaging builds the real engine,
Slint editor and Game Bar host, validates the MSIX manifest and includes the
Microsoft-signed x64 VCLibs dependency. It never installs the result. A PR package
is unsigned, has its updater disabled and cannot be promoted to a release.
`-Offline` on build/package scripts uses previously verified cached dependencies.
Choose a fresh package output directory rather than overwriting an artifact.

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
native click-through, DPI, real-game compatibility, Slint accessibility or signed
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
[PR template](.github/pull_request_template.md). Justify rejected findings with
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
   signed Windows package containing engine, editor and Game Bar widget, its
   updater signature, `latest.json` and SHA-256 checksums. Verify
   downloaded assets and clean-install/upgrade behavior before publication.
   Never promote PR artifacts or silently rebuild a different revision.
5. Generate exactly one `Changelog:` comparison link and one issue-report link
   in the release body; do not duplicate those footers in changelog entries.
   Only stable releases advance latest. Never overwrite published versions/tags.

Keep credentials out of PR jobs. Configure scoped publication secrets through the
platform, never by copying a local login token. Updater signing is distinct from
Windows package signing; do not claim either until configured and verified.
Settings-driven checks/downloads and explicit Install and restart are part of the
complete plan; unattended installation is not. Follow the architecture's signed
version, artifact provenance, deployment and recovery contracts.

### Maintainer commands and signing configuration

Use **Prepare release** with a stable `MAJOR.MINOR.PATCH` and reviewed Markdown
bullets. It updates Cargo.toml/Cargo.lock and CHANGELOG.md and opens a draft
`feat/release-VERSION` PR. Its scoped `PUBLISHER_TOKEN` needs repository contents
and pull-request write permission so normal PR validation runs; do not substitute
a local login credential. Notes omit the generated comparison and issue footers.

Configure repository variables `CROSSHAIR_UPDATE_PUBLIC_KEY` (the project's
Minisign base64 public key) and `CROSSHAIR_PACKAGE_PUBLISHER` (the exact Windows
signing-certificate subject) before validating a release candidate on main.
Both values are compiled into the engine and checked by packaging through
`crosshair-engine.exe --build-info`. They are public configuration, not secrets.
Changing either requires a newly validated artifact. Without them, validation
produces only an updater-disabled development package. Local release-candidate
builds explicitly use `./scripts/package-windows.ps1 -ReleaseCandidate` with both
variables set; no signing key is needed to build or validate that candidate.

The protected `release` environment supplies `WINDOWS_CERTIFICATE_PFX_BASE64`,
`WINDOWS_CERTIFICATE_PASSWORD` and the complete, unencrypted project-specific
`MINISIGN_PRIVATE_KEY` file as secrets. Provision and back up signing keys outside
this repository. **Prepare signed draft release** accepts the stable version and
the latest successful Validation run ID for exact current main. It rejects old,
failed, cancelled, superseded, expired, dirty or PR artifacts. The workflow copies
and signs that immutable MSIX without rebuilding its contents. The signing
certificate is imported only into the ephemeral runner's personal store and
removed afterward; nothing adds a root/trust certificate or enables Developer
Mode. Windows must already trust the production certificate chain.

The draft contains `crosshair-rs-VERSION-windows-x64.msix`, its `.minisig`,
`latest.json`, `SHA256SUMS`, `Microsoft.VCLibs.x64.14.00.appx` and `Install.ps1`.
The workflow downloads and verifies all six assets. It does not publish the
draft. Complete clean-machine installation, signed N-to-N+1 update, cancellation,
interrupted deployment and restart checks before a separately authorized GitHub
publication. The updater consumes stable releases only; local preview packages
use `.Preview` identity plus explicit `-PreviewBuild` and have updates disabled.

For a downloaded signed release, place its assets together and run
`powershell -NoProfile -File .\Install.ps1`. Windows verifies package signatures
and trust and installs the pinned x64 framework as a dependency. This command
does not elevate, change trust or install a service. An unsigned local package
requires a separately authorized development registration or trusted test signing;
the build scripts deliberately perform neither.
