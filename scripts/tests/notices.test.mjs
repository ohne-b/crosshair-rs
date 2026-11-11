import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, relative } from 'node:path';
import { reachablePackages, licenseFiles, renderNotices } from '../generate-notices.mjs';

test('notices follow reachable normal/build edges and exclude dev/unreachable crates', () => {
  const names = ['crosshair-engine', 'crosshair-editor', 'crosshair-core', 'runtime', 'build-tool', 'dev-only', 'other-platform'];
  const packages = names.map((name, index) => ({ id: String(index), name, version: '1.0.0' }));
  const dep = (id, kind = null) => ({ pkg: String(id), dep_kinds: [{ kind }] });
  const metadata = { packages, workspace_members: ['0', '1', '2'], resolve: { nodes: packages.map(value => ({ id: value.id, deps:
    value.id === '0' ? [dep(2), dep(5, 'dev')] : value.id === '1' ? [dep(4, 'build')] : value.id === '2' ? [dep(3)] : [] })) } };
  assert.deepEqual(reachablePackages(metadata).map(value => value.name), ['build-tool', 'runtime']);
  assert.throws(() => reachablePackages({ ...metadata, workspace_members: ['0'] }));
});

test('notices preserve actual files deterministically without local paths', () => {
  const directory = mkdtempSync(join(tmpdir(), 'crosshair-notices-'));
  try {
    mkdirSync(join(directory, 'LICENSES'));
    writeFileSync(join(directory, 'LICENSE-MIT'), 'Copyright fixture authors\r\nFixture license text.\r\n');
    writeFileSync(join(directory, 'LICENSES/extra.txt'), 'Additional fixture terms with ``` literal fences.\n');
    writeFileSync(join(directory, 'README.md'), 'Not license material.');
    const pkg = { name: 'fixture', version: '1.2.3', license: 'MIT', repository: 'https://example.com/fixture', source: 'registry+fixture', manifest_path: join(directory, 'Cargo.toml') };
    assert.deepEqual(licenseFiles(directory).map(path => relative(directory, path).replaceAll('\\', '/')), ['LICENSE-MIT', 'LICENSES/extra.txt']);
    const text = renderNotices([pkg]);
    assert(text.includes('Copyright fixture authors\nFixture license text.'));
    assert(text.includes('````text\nAdditional fixture terms with ``` literal fences.'));
    assert(!text.includes(directory) && !text.includes('Not license material'));
    assert.equal(text, renderNotices([pkg]));
    assert.throws(() => renderNotices([pkg, pkg]));
  } finally {
    // The resolved target is the exact temporary directory created above.
    assert.equal(resolve(directory), directory);
    assert(directory.startsWith(resolve(tmpdir())));
    rmSync(directory, { recursive: true });
  }
});

test('missing license text fails until an exact-version sourced override is provided', () => {
  const directory = mkdtempSync(join(tmpdir(), 'crosshair-notices-'));
  try {
    const pkg = { name: 'fixture', version: '1.2.3', license: 'MIT', manifest_path: join(directory, 'Cargo.toml') };
    assert.throws(() => renderNotices([pkg]), /fixture@1.2.3/);
    writeFileSync(join(directory, 'upstream.txt'), 'The exact upstream fixture copyright and terms.');
    const override = { 'fixture@1.2.3': { files: ['upstream.txt'], source: 'https://example.com/fixture/v1.2.3/LICENSE', reason: 'Fixture package omitted its upstream license.' } };
    assert(renderNotices([pkg], override, directory).includes('The exact upstream fixture copyright and terms.'));
    assert.throws(() => renderNotices([pkg], { 'fixture@1.2.4': override['fixture@1.2.3'] }, directory));
    assert.throws(() => licenseFiles(directory, '../outside-license.txt'));
  } finally {
    assert.equal(resolve(directory), directory);
    assert(directory.startsWith(resolve(tmpdir())));
    rmSync(directory, { recursive: true });
  }
});
