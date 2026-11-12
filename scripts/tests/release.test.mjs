import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { releaseVersion, packageVersion, releaseNotes, releaseBody, notesFor, validateBuild, validateRun, manifest, metadata, verifyAssets } from '../release.mjs';

test('release versions preserve SemVer order in unsigned MSIX package versions', () => {
  const versions = ['0.1.0-alpha.0', '0.1.0-alpha.1', '0.1.0-alpha.19999', '0.1.0-beta.0', '0.1.0-beta.19999', '0.1.0-rc.0', '0.1.0-rc.19999', '0.1.0', '0.1.1-alpha.0'];
  let previous = null;
  let mapped = null;
  for (const version of versions) {
    assert.equal(releaseVersion(version, previous), version);
    const next = packageVersion(version).split('.').reduce((value, part) => (value << 16n) | BigInt(part), 0n);
    if (mapped !== null) assert.ok(next > mapped);
    previous = version; mapped = next;
  }
  assert.equal(packageVersion('0.1.0-alpha.1'), '0.1.0.1');
  assert.equal(packageVersion('0.1.0'), '0.1.0.65535');
  for (const value of ['v1.2.3', '01.2.3', '1.2.3-preview.1', '1.2.3-alpha.20000', '1.2.3-rc.01', '1.2.3+build', '65536.0.0', '1.2']) assert.throws(() => releaseVersion(value));
  assert.throws(() => releaseVersion('0.1.0-alpha.1', '0.1.0'));
});
test('notes have a single generated comparison and issue footer', () => {
  assert.equal(releaseNotes('- Fixed a bug.\r\n- Improved rendering.'), '- Fixed a bug.\n- Improved rendering.');
  for (const value of ['', '# Changes', 'Changelog: link', '- Changelog: duplicate', '- Fixed.\nReport an issue']) assert.throws(() => releaseNotes(value));
  const body = releaseBody('- Fixed.', '1.2.3', '1.2.2');
  assert.equal(body.match(/Changelog:/g).length, 1);
  assert.equal(body.match(/open an issue/g).length, 1);
  assert.ok(body.endsWith('Please [open an issue](https://github.com/ohne-b/crosshair-rs/issues/new) to report bugs or request features.\n'));
  const first = notesFor('0.1.0-alpha.1', '# Changelog\n\n## [v0.1.0-alpha.1](https://github.com/ohne-b/crosshair-rs/releases/tag/v0.1.0-alpha.1) 2026-11-12\n\n- First alpha.\n');
  assert.ok(first.includes('Changelog: https://github.com/ohne-b/crosshair-rs/commits/v0.1.0-alpha.1'));
});
test('artifact promotion binds clean source, compiled trust and package hash', () => {
  const build = { schemaVersion: 1, version: '1.2.3', packageVersion: '1.2.3.65535', commit: 'a'.repeat(40), dirty: false,
    platform: 'windows-x86_64-msix', signed: false, releaseCandidate: true, sha256: 'b'.repeat(64),
    package: 'crosshair-rs-1.2.3-windows-x64.unsigned.msix', configuration: { version: '1.2.3', packageName: 'OhneB.crosshair-rs',
      updatesConfigured: true, updatePublicKey: 'fixture', packagePublisher: 'CN=fixture' } };
  const validate = value => validateBuild(value, '1.2.3', 'a'.repeat(40), 'fixture', 'CN=fixture');
  validate(build);
  for (const patch of [{ dirty: true }, { signed: true }, { commit: 'c'.repeat(40) }, { releaseCandidate: false }, { sha256: 'wrong' },
    { configuration: { ...build.configuration, updatePublicKey: 'wrong' } }, { package: '../elsewhere.msix' }]) assert.throws(() => validate({ ...build, ...patch }));
});
test('only latest successful canonical-main validation can be promoted', () => {
  const run = { id: 42, run_number: 5, run_attempt: 2, repository: { full_name: 'ohne-b/crosshair-rs' },
    head_repository: { full_name: 'ohne-b/crosshair-rs' }, head_branch: 'main', head_sha: 'a'.repeat(40),
    event: 'push', status: 'completed', conclusion: 'success', path: '.github/workflows/validation.yml' };
  assert.equal(validateRun(run, [run], run.head_sha), `windows-${run.head_sha}-42-2`);
  for (const patch of [{ head_branch: 'feature' }, { event: 'pull_request' }, { conclusion: 'cancelled' },
    { path: '.github/workflows/other.yml' }, { head_repository: { full_name: 'attacker/crosshair-rs' } }]) assert.throws(() => validateRun({ ...run, ...patch }, [run], run.head_sha));
  assert.throws(() => validateRun(run, [{ ...run, id: 43, run_number: 6, conclusion: 'failure' }, run], run.head_sha));
  assert.throws(() => validateRun(run, [{ ...run, run_attempt: 3, conclusion: 'cancelled' }], run.head_sha));
  const retry = { ...run, run_attempt: 3 };
  assert.throws(() => validateRun(retry, [retry], run.head_sha, `windows-${run.head_sha}-42-2`));
  assert.equal(validateRun(retry, [retry], run.head_sha, `windows-${run.head_sha}-42-3`), `windows-${run.head_sha}-42-3`);
});
test('latest manifest rejects wrong signed version and legacy signatures', () => {
  const data = Buffer.alloc(74); data.write('ED');
  const signature = `untrusted comment: fixture\n${data.toString('base64')}\ntrusted comment: version:1.2.3\n${Buffer.alloc(64).toString('base64')}\n`;
  const result = manifest('1.2.3', signature, '2026-11-11T12:00:00Z');
  assert.equal(result.notes, '[Check release notes on GitHub](https://github.com/ohne-b/crosshair-rs/releases/tag/v1.2.3)');
  assert.equal(Buffer.from(result.platforms['windows-x86_64-msix'].signature, 'base64').toString(), signature);
  assert.throws(() => manifest('1.2.4', signature, result.pub_date));
  assert.throws(() => manifest('1.2.3', signature, '2026-02-31T12:00:00Z'));
  data.write('Ed');
  assert.throws(() => manifest('1.2.3', signature.replace(signature.split('\n')[1], data.toString('base64')), result.pub_date));
});

test('release asset verification rejects changed installers, missing files and extra files', () => {
  const directory = mkdtempSync(join(tmpdir(), 'crosshair-release-'));
  try {
    const version = '0.1.0-alpha.1';
    const filename = `crosshair-rs-${version}-windows-x64.msix`;
    const data = Buffer.alloc(74); data.write('ED');
    const signature = `untrusted comment: fixture\n${data.toString('base64')}\ntrusted comment: version:${version}\n${Buffer.alloc(64).toString('base64')}\n`;
    for (const name of [filename, 'Microsoft.VCLibs.x64.14.00.appx', 'Install.ps1', 'minisign.exe', 'Minisign-LICENSE.txt']) writeFileSync(join(directory, name), 'fixture');
    writeFileSync(join(directory, `${filename}.minisig`), signature);
    metadata(directory, version);
    verifyAssets(directory, version);
    writeFileSync(join(directory, 'Install.ps1'), 'changed');
    assert.throws(() => verifyAssets(directory, version));
    writeFileSync(join(directory, 'Install.ps1'), 'fixture');
    const manifestPath = join(directory, 'latest.json');
    const original = readFileSync(manifestPath);
    writeFileSync(manifestPath, original.toString().replace('0.1.0-alpha.1', '0.1.0-alpha.2'));
    assert.throws(() => verifyAssets(directory, version));
    writeFileSync(manifestPath, original);
    writeFileSync(join(directory, 'extra.exe'), 'extra');
    assert.throws(() => verifyAssets(directory, version));
    rmSync(join(directory, 'extra.exe'));
    rmSync(join(directory, 'minisign.exe'));
    assert.throws(() => verifyAssets(directory, version));
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
