import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { matchesGlob } from 'node:path/posix';
import { parse } from 'yaml';

const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));

function runsFor(filter, files) {
  if (filter['paths-ignore']) {
    return files.some(file => !filter['paths-ignore'].some(pattern => matchesGlob(file, pattern)));
  }
  return files.some(file => filter.paths.reduce((included, pattern) => {
    const exclude = pattern.startsWith('!');
    return matchesGlob(file, exclude ? pattern.slice(1) : pattern) ? !exclude : included;
  }, false));
}

test('website and Markdown changes avoid application CI without skipping mixed application changes', () => {
  const application = workflow('validation');
  const website = workflow('website-validation');
  const cases = [
    [['website/src/components/Hero.astro'], false, true],
    [['website/package-lock.json'], false, true],
    [['website/public/assets/logo.svg'], false, true],
    [['assets/icons/brand-discord.svg'], true, true],
    [['README.md', 'docs/architecture.md', 'AGENTS.md'], false, false],
    [['website/README.md'], false, false],
    [['website/src/styles/global.css', 'docs/usage.md'], false, true],
    [['.github/workflows/website-validation.yml'], false, true],
    [['crates/core/src/lib.rs', 'docs/architecture.md'], true, false],
    [['apps/engine/src/main.rs', 'website/src/pages/index.astro'], true, true],
    [['Cargo.lock'], true, false],
    [['scripts/release.mjs'], true, false],
    [['.github/workflows/validation.yml'], true, false],
  ];
  for (const event of ['pull_request', 'push']) {
    for (const [files, appRuns, websiteRuns] of cases) {
      assert.equal(runsFor(application.on[event], files), appRuns, `${event} application: ${files}`);
      assert.equal(runsFor(website.on[event], files), websiteRuns, `${event} website: ${files}`);
    }
  }
});

test('deployment and release workflows stay manual while validation remains manually available', () => {
  for (const name of ['deploy-website', 'prepare-release', 'publish-release']) {
    assert.deepEqual(Object.keys(workflow(name).on), ['workflow_dispatch']);
  }
  for (const name of ['validation', 'website-validation']) {
    assert.ok(Object.hasOwn(workflow(name).on, 'workflow_dispatch'));
    assert.deepEqual(workflow(name).on.push.branches, ['main']);
  }
});
