import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import semver from 'semver';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repository = 'ohne-b/crosshair-rs';
const url = `https://github.com/${repository}`;
const run = (command, args, options = {}) => execFileSync(command, args, { cwd: root, encoding: 'utf8', ...options }).trim();
const json = path => JSON.parse(readFileSync(path, 'utf8'));
const writeJson = (path, value) => writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
export const digest = bytes => createHash('sha256').update(bytes).digest('hex');

export function stableVersion(value) {
  if (semver.valid(value) !== value || semver.prerelease(value) || semver.parse(value).build.length ||
      [semver.major(value), semver.minor(value), semver.patch(value)].some(part => part > 65535)) {
    throw new Error('Release version must be canonical stable MAJOR.MINOR.PATCH within MSIX limits.');
  }
  return value;
}

export function releaseNotes(notes) {
  const text = notes.replaceAll('\r\n', '\n').trim();
  if (!text || text.length > 16000 || /Changelog:|report (?:an )?issue/i.test(text) ||
      text.split('\n').some(line => line && !line.startsWith('- '))) {
    throw new Error('Release notes must contain concise Markdown bullets, without headings or generated footers.');
  }
  return text;
}

export function releaseBody(notes, version, previous) {
  stableVersion(version);
  if (!/^(?:v\d+\.\d+\.\d+|[a-f0-9]{40})$/.test(previous)) throw new Error('Invalid previous release reference.');
  return `${releaseNotes(notes)}\n\nChangelog: ${url}/compare/${previous}...v${version}\n\n[Report an issue](${url}/issues/new)\n`;
}

export function validateBuild(build, version, commit, publicKey, publisher) {
  stableVersion(version);
  if (build.schemaVersion !== 1 || build.version !== version || build.packageVersion !== `${version}.0` ||
      build.commit !== commit || build.dirty !== false || build.platform !== 'windows-x86_64-msix' ||
      build.signed !== false || build.releaseCandidate !== true || !/^[a-f0-9]{64}$/.test(build.sha256) ||
      build.package !== `crosshair-rs-${version}-windows-x64.unsigned.msix` ||
      build.configuration?.updatesConfigured !== true || build.configuration.version !== version ||
      build.configuration.packageName !== 'OhneB.crosshair-rs' || !publicKey || !publisher ||
      build.configuration.updatePublicKey !== publicKey || build.configuration.packagePublisher !== publisher) {
    throw new Error('Build provenance, product version or compiled update trust differs from the requested release.');
  }
}

export function validateRun(selected, candidates, main) {
  if (selected.repository?.full_name !== repository || selected.head_repository?.full_name !== repository ||
      selected.head_branch !== 'main' || selected.head_sha !== main || !['push', 'workflow_dispatch'].includes(selected.event) ||
      selected.status !== 'completed' || selected.conclusion !== 'success' || selected.path !== '.github/workflows/validation.yml') {
    throw new Error('Publication requires successful Validation on exact canonical main.');
  }
  const latest = candidates.filter(value => value.head_sha === main && value.head_branch === 'main' &&
      ['push', 'workflow_dispatch'].includes(value.event)).sort((a, b) => b.run_number - a.run_number)[0];
  if (!latest || latest.id !== selected.id || latest.run_attempt !== selected.run_attempt || latest.conclusion !== 'success') {
    throw new Error('A newer validation run or attempt supersedes this artifact. Revalidate main.');
  }
  return `windows-${main}-${selected.id}-${selected.run_attempt}`;
}

export function manifest(version, signature, date) {
  stableVersion(version);
  const lines = signature.trimEnd().split(/\r?\n/);
  if (signature.length > 3072 || lines.length !== 4 || lines[2] !== `trusted comment: version:${version}` ||
      !lines[0].startsWith('untrusted comment: ') || Buffer.from(lines[1], 'base64').length !== 74 ||
      Buffer.from(lines[3], 'base64').length !== 64 || Buffer.from(lines[1], 'base64').subarray(0, 2).toString() !== 'ED') {
    throw new Error('Expected a prehashed Minisign signature authenticating the exact release version.');
  }
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(date) || !Number.isFinite(Date.parse(date)) ||
      new Date(date).toISOString().replace('.000Z', 'Z') !== date) throw new Error('Invalid publication date.');
  return { schemaVersion: 1, version, notes: `${url}/releases/tag/v${version}`, pub_date: date,
    platforms: { 'windows-x86_64-msix': { url: `${url}/releases/download/v${version}/crosshair-rs-${version}-windows-x64.msix`,
      signature: Buffer.from(signature).toString('base64') } } };
}

function version() {
  const metadata = JSON.parse(run('cargo', ['metadata', '--locked', '--no-deps', '--format-version', '1']));
  const versions = new Set(metadata.packages.filter(value => metadata.workspace_members.includes(value.id)).map(value => value.version));
  if (versions.size !== 1) throw new Error('All workspace packages must inherit the product version.');
  return [...versions][0];
}

function prepare(next, noteFile) {
  stableVersion(next);
  const current = version();
  if (semver.lt(next, current)) throw new Error('Release version cannot decrease.');
  const notes = releaseNotes(readFileSync(noteFile, 'utf8'));
  const changelog = join(root, 'CHANGELOG.md');
  const old = existsSync(changelog) ? readFileSync(changelog, 'utf8') : '# Changelog\n';
  if (old.includes(`## [v${next}]`)) throw new Error('Version already has release notes.');
  if (next !== current) {
    const path = join(root, 'Cargo.toml');
    const source = readFileSync(path, 'utf8');
    const changed = source.replace(/(\[workspace\.package\][\s\S]*?^version\s*=\s*")[^"]+("\s*$)/m,
      (_match, before, after) => `${before}${next}${after}`);
    if (source === changed) throw new Error('Workspace version was not updated.');
    const lock = readFileSync(join(root, 'Cargo.lock'));
    try {
      writeFileSync(path, changed);
      run('cargo', ['update', '--workspace', '--offline']);
      if (version() !== next) throw new Error('Workspace version synchronization failed.');
    } catch (error) { writeFileSync(path, source); writeFileSync(join(root, 'Cargo.lock'), lock); throw error; }
  }
  const entry = `## [v${next}](${url}/releases/tag/v${next}) ${new Date().toISOString().slice(0, 10)}\n\n${notes}\n`;
  writeFileSync(changelog, `# Changelog\n\n${entry}\n${old.replace(/^# Changelog\s*/, '').trim()}\n`.trimEnd() + '\n');
}

function notesFor(value) {
  stableVersion(value);
  const content = readFileSync(join(root, 'CHANGELOG.md'), 'utf8');
  const heading = `## [v${value}](${url}/releases/tag/v${value}) `;
  const start = content.indexOf(heading);
  if (start < 0 || content.indexOf(heading, start + 1) >= 0) throw new Error('Exactly one changelog entry is required.');
  const body = content.slice(content.indexOf('\n', start) + 1).split(/^## /m)[0];
  return releaseNotes(body);
}

function api(path) { return JSON.parse(run('gh', ['api', path])); }

function gate(id, requested) {
  stableVersion(requested);
  if (!/^\d+$/.test(id) || version() !== requested) throw new Error('Invalid run ID or workspace version.');
  notesFor(requested);
  const main = api(`repos/${repository}/commits/main`).sha;
  if (run('git', ['rev-parse', 'HEAD']) !== main || process.env.GITHUB_REF !== 'refs/heads/main') throw new Error('Run this workflow from current canonical main.');
  const selected = api(`repos/${repository}/actions/runs/${id}`);
  const candidates = api(`repos/${repository}/actions/workflows/validation.yml/runs?branch=main&head_sha=${main}&per_page=100`).workflow_runs;
  const artifact = validateRun(selected, candidates, main);
  const artifacts = api(`repos/${repository}/actions/runs/${id}/artifacts?per_page=100`).artifacts.filter(value => value.name === artifact && !value.expired);
  if (artifacts.length !== 1) throw new Error('The validated immutable artifact is missing or expired. Revalidate.');
  if (api(`repos/${repository}/releases?per_page=100`).some(value => value.tag_name === `v${requested}`) ||
      api(`repos/${repository}/git/matching-refs/tags/v${requested}`).some(value => value.ref === `refs/tags/v${requested}`)) {
    throw new Error('Release or tag already exists; it will not be overwritten.');
  }
  writeFileSync(process.env.GITHUB_OUTPUT, `artifact=${artifact}\ncommit=${main}\n`, { flag: 'a' });
}

function metadata(directory, value) {
  stableVersion(value);
  const filename = `crosshair-rs-${value}-windows-x64.msix`;
  const signature = readFileSync(join(directory, `${filename}.minisig`), 'utf8');
  writeJson(join(directory, 'latest.json'), manifest(value, signature, new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')));
  const files = [filename, `${filename}.minisig`, 'latest.json', 'Microsoft.VCLibs.x64.14.00.appx', 'Install.ps1'];
  writeFileSync(join(directory, 'SHA256SUMS'), files.map(name => `${digest(readFileSync(join(directory, name)))}  ${name}\n`).join(''));
}

function verifyAssets(directory, value) {
  stableVersion(value);
  const filename = `crosshair-rs-${value}-windows-x64.msix`;
  const expected = [filename, `${filename}.minisig`, 'latest.json', 'Microsoft.VCLibs.x64.14.00.appx', 'Install.ps1'].sort();
  const lines = readFileSync(join(directory, 'SHA256SUMS'), 'utf8').trimEnd().split('\n');
  const names = [];
  for (const line of lines) {
    const match = /^([a-f0-9]{64})  ([a-zA-Z0-9._-]+)$/.exec(line);
    if (!match || names.includes(match[2]) || digest(readFileSync(join(directory, match[2]))) !== match[1]) throw new Error('Release checksum verification failed.');
    names.push(match[2]);
  }
  if (JSON.stringify(names.sort()) !== JSON.stringify(expected) ||
      JSON.stringify(readdirSync(directory).sort()) !== JSON.stringify([...expected, 'SHA256SUMS'].sort())) throw new Error('Unexpected or missing release assets.');
  const actual = json(join(directory, 'latest.json'));
  const generated = manifest(value, readFileSync(join(directory, `${filename}.minisig`), 'utf8'), actual.pub_date);
  if (JSON.stringify(actual) !== JSON.stringify(generated)) throw new Error('Updater metadata does not match the signed release.');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...args] = process.argv.slice(2);
  try {
    switch (command) {
      case 'version': console.log(version()); break;
      case 'prepare': prepare(...args); break;
      case 'gate': gate(...args); break;
      case 'body': writeFileSync(args[2], releaseBody(notesFor(args[0]), args[0], args[1])); break;
      case 'metadata': metadata(...args); break;
      case 'verify-assets': verifyAssets(...args); break;
      case 'verify-build': {
        const [directory, requested, commit] = args;
        const build = json(join(directory, 'build.json'));
        validateBuild(build, requested, commit, process.env.CROSSHAIR_UPDATE_PUBLIC_KEY, process.env.CROSSHAIR_PACKAGE_PUBLISHER);
        if (digest(readFileSync(join(directory, build.package))) !== build.sha256) throw new Error('Immutable package hash mismatch.');
        break;
      }
      default: throw new Error('Unknown release command.');
    }
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
