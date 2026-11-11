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

The repository currently contains documentation and artwork only. There is no
Cargo workspace, Slint build or Game Bar host yet. Do not invent passing
application checks.

For groundwork changes, run `git diff --check` (and `git diff --cached --check`
for newly staged files), inspect the complete diff, verify relative links and
commands, parse changed workflow files and SVGs, and check factual claims against
their sources. The [Validation workflow](.github/workflows/validation.yml) checks
tracked-file whitespace on PRs, main pushes and manual runs. It does not validate
application behavior, external links or native compatibility.

Introduce build/test checks alongside each runnable component. The intended
baseline is Rust formatting, Clippy and tests with the committed Cargo lockfile,
plus Slint compile/UI checks, Game Bar host/package builds and meaningful behavior
tests once those components exist. Add actual commands here with their manifests.
Use Windows runners for native checks; browser tests of a design preview cannot
verify Win32 input, DPI, tray behavior, Slint accessibility or package deployment.

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

There is no release workflow yet because no executable or installer exists. Add
packaging automation with the packaging milestone, following these contracts:

1. Prepare a version change on `feat/release-VERSION` through a draft PR. Once
   Cargo exists, `[workspace.package].version` owns the product version; product
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
version, artifact provenance, deployment and recovery contracts. Document actual
artifact names, signing and recovery commands when implemented.
