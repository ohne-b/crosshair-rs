import test from 'node:test';
import assert from 'node:assert/strict';
import { stableVersion, releaseNotes, releaseBody, validateBuild, validateRun, manifest } from '../release.mjs';

test('release versions reject aliases, previews, metadata and MSIX overflow', () => {
  assert.equal(stableVersion('1.2.3'), '1.2.3');
  for (const value of ['v1.2.3', '01.2.3', '1.2.3-rc.1', '1.2.3+build', '65536.0.0', '1.2']) assert.throws(() => stableVersion(value));
});
test('notes have a single generated comparison and issue footer', () => {
  assert.equal(releaseNotes('- Fixed a bug.\r\n- Improved rendering.'), '- Fixed a bug.\n- Improved rendering.');
  for (const value of ['', '# Changes', 'Changelog: link', '- Changelog: duplicate', '- Fixed.\nReport an issue']) assert.throws(() => releaseNotes(value));
  const body = releaseBody('- Fixed.', '1.2.3', 'v1.2.2');
  assert.equal(body.match(/Changelog:/g).length, 1);
  assert.equal(body.match(/Report an issue/g).length, 1);
});
test('artifact promotion binds clean source, compiled trust and package hash', () => {
  const build = { schemaVersion: 1, version: '1.2.3', packageVersion: '1.2.3.0', commit: 'a'.repeat(40), dirty: false,
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
});
test('latest manifest rejects wrong signed version and legacy signatures', () => {
  const data = Buffer.alloc(74); data.write('ED');
  const signature = `untrusted comment: fixture\n${data.toString('base64')}\ntrusted comment: version:1.2.3\n${Buffer.alloc(64).toString('base64')}\n`;
  const result = manifest('1.2.3', signature, '2026-11-11T12:00:00Z');
  assert.equal(Buffer.from(result.platforms['windows-x86_64-msix'].signature, 'base64').toString(), signature);
  assert.throws(() => manifest('1.2.4', signature, result.pub_date));
  assert.throws(() => manifest('1.2.3', signature, '2026-02-31T12:00:00Z'));
  data.write('Ed');
  assert.throws(() => manifest('1.2.3', signature.replace(signature.split('\n')[1], data.toString('base64')), result.pub_date));
});
