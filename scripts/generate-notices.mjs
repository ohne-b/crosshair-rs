import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, readdirSync, lstatSync, realpathSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, resolve, relative, isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const order = (left, right) => left < right ? -1 : left > right ? 1 : 0;
const normalized = path => path.replaceAll('\\', '/');

export function reachablePackages(metadata, compiled) {
  if (!metadata.resolve?.nodes || !metadata.packages || !metadata.workspace_members) throw new Error('Cargo metadata has no resolved dependency graph.');
  const nodes = new Map(metadata.resolve.nodes.map(value => [value.id, value]));
  const packages = new Map(metadata.packages.map(value => [value.id, value]));
  const workspace = new Set(metadata.workspace_members);
  const pending = ['crosshair-engine', 'crosshair-editor'].map(name => {
    const matches = metadata.packages.filter(value => value.name === name && workspace.has(value.id));
    if (matches.length !== 1) throw new Error(`Missing unique workspace application: ${name}`);
    return matches[0].id;
  });
  const seen = new Set();
  while (pending.length) {
    const id = pending.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    const node = nodes.get(id);
    if (!node || !packages.has(id)) throw new Error('Cargo dependency graph is incomplete.');
    for (const dependency of node.deps) {
      const pkg = packages.get(dependency.pkg);
      if (!pkg) throw new Error('Cargo dependency graph is incomplete.');
      if ((!compiled || compiled.has(`${pkg.name}@${pkg.version}`)) &&
          dependency.dep_kinds.some(value => value.kind === null || value.kind === 'build')) pending.push(dependency.pkg);
    }
  }
  return [...seen].filter(id => !workspace.has(id)).map(id => packages.get(id))
    .sort((left, right) => order(left.name, right.name) || order(left.version, right.version));
}

function contained(base, path) {
  const part = relative(base, path);
  return part !== '..' && !part.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) && !isAbsolute(part);
}

export function licenseFiles(packageRoot, declared) {
  packageRoot = realpathSync(packageRoot);
  const files = new Set();
  function add(path, depth = 0) {
    if (depth > 8 || !contained(packageRoot, path)) throw new Error('License path escapes its source package or is too deeply nested.');
    const stat = lstatSync(path);
    if (stat.isSymbolicLink() || !contained(packageRoot, realpathSync(path))) throw new Error('License symlinks require an explicit reviewed override.');
    if (stat.isFile()) {
      if (stat.size === 0 || stat.size > 2 * 1024 * 1024 || files.size >= 128) throw new Error('License file is empty or exceeds the material limit.');
      files.add(path);
    } else if (stat.isDirectory()) {
      for (const name of readdirSync(path).sort(order)) add(join(path, name), depth + 1);
    }
  }
  for (const name of readdirSync(packageRoot).sort(order)) {
    if (/^(?:licen[cs]e|copying|copyright|unlicense|notice)(?:$|[._-]|s$)/i.test(name)) add(join(packageRoot, name));
  }
  if (declared) add(resolve(packageRoot, declared));
  return [...files].sort((a, b) => order(normalized(relative(packageRoot, a)), normalized(relative(packageRoot, b))));
}

function licenseText(path) {
  const text = new TextDecoder('utf-8', { fatal: true }).decode(readFileSync(path)).replaceAll('\r\n', '\n').trimEnd();
  if (!text.trim() || text.includes('\0')) throw new Error('License material must contain readable UTF-8 text.');
  return text;
}

function literal(value) {
  return String(value).replaceAll('`', '\\`').replaceAll('\r', '').replaceAll('\n', ' ');
}

function sourceLink(value) {
  if (!value) return null;
  const url = new URL(value);
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || /[<>\r\n]/.test(value)) throw new Error('Repository metadata must be a public HTTP(S) URL.');
  return url.href;
}

export function renderNotices(packages, overrides = {}, overrideRoot = root) {
  const sections = ['# Rust dependency notices\n\nGenerated from Cargo.lock for x86_64-pc-windows-msvc. Normal and build dependencies reachable from crosshair-engine and crosshair-editor are included. Upstream license expressions describe the offered terms; see [NOTICE.md](../NOTICE.md) for the selected Slint desktop license.\n'];
  const missing = [];
  const used = new Set();
  const identities = new Set();
  for (const pkg of [...packages].sort((left, right) => order(left.name, right.name) || order(left.version, right.version))) {
    const key = `${pkg.name}@${pkg.version}`;
    if (identities.has(key)) throw new Error(`Multiple sources supply ${key}; resolve the source ambiguity before packaging.`);
    identities.add(key);
    let material;
    try {
      const packageRoot = realpathSync(dirname(pkg.manifest_path));
      const override = overrides[key];
      if (override) {
        if (!Array.isArray(override.files) || !override.files.length || !override.reason || !sourceLink(override.source)) throw new Error('An override needs exact-version files, reason and original source URL.');
        used.add(key);
        material = override.files.map(value => {
          if (typeof value !== 'string' || isAbsolute(value)) throw new Error('Override files must be repository-relative.');
          const path = realpathSync(resolve(overrideRoot, value));
          if (!contained(realpathSync(overrideRoot), path) || !lstatSync(path).isFile()) throw new Error('Override file is outside the repository.');
          return { name: normalized(value), text: licenseText(path) };
        });
      } else {
        material = licenseFiles(packageRoot, pkg.license_file).map(path => ({ name: normalized(relative(packageRoot, path)), text: licenseText(path) }));
      }
      if (!material.length) throw new Error('No upstream license file was supplied.');
      const repository = sourceLink(pkg.repository);
      let heading = `\n## ${literal(pkg.name)} ${literal(pkg.version)}\n\nDeclared license: ${literal(pkg.license ?? 'See supplied license material')}.\n`;
      if (repository) heading += `\nRepository: <${repository}>.\n`;
      if (pkg.source?.startsWith('registry+')) heading += `\nVersioned source: <https://crates.io/crates/${encodeURIComponent(pkg.name)}/${encodeURIComponent(pkg.version)}>.\n`;
      if (override) heading += `\nReviewed material override: ${literal(override.reason)} Source: <${sourceLink(override.source)}>.\n`;
      for (const item of material) {
        const fence = '`'.repeat(Math.max(3, ...[...item.text.matchAll(/`+/g)].map(match => match[0].length + 1)));
        heading += `\n### ${literal(item.name)}\n\n${fence}text\n${item.text}\n${fence}\n`;
      }
      sections.push(heading);
    } catch (error) {
      // Keep machine paths out of build logs and generated artifacts.
      missing.push(`${key}: missing, unreadable or unsupported license material (${error.code ?? 'review required'}).`);
    }
  }
  if (missing.length) throw new Error(`${missing.join('\n')}\nProvide original upstream files through an exact-version reviewed override; do not invent copyright or substitute an unrelated SPDX template.`);
  const unused = Object.keys(overrides).filter(key => !used.has(key));
  if (unused.length) throw new Error(`Stale license overrides: ${unused.join(', ')}`);
  return sections.join('');
}

export function generate(output, overrideFile) {
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc'],
    { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] }));
  const overrides = overrideFile ? JSON.parse(readFileSync(overrideFile, 'utf8')) : {};
  if (typeof overrides !== 'object' || overrides === null || Array.isArray(overrides)) throw new Error('License overrides must be an object keyed by exact crate@version.');
  // Metadata also resolves optional feature combinations which the application does not build.
  const tree = execFileSync('cargo', ['tree', '--locked', '--color', 'never', '--target', 'x86_64-pc-windows-msvc',
    '--edges', 'normal,build', '--prefix', 'none', '--format', '{p}', '-p', 'crosshair-engine', '-p', 'crosshair-editor'],
    { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] });
  const compiled = new Set(tree.trim().split(/\r?\n/).filter(Boolean).map(line => {
    const match = /^([A-Za-z0-9_-]+) v([^ ]+)(?: |$)/.exec(line);
    if (!match) throw new Error('Unexpected Cargo build tree entry.');
    return `${match[1]}@${match[2]}`;
  }));
  const packages = reachablePackages(metadata, compiled);
  const text = renderNotices(packages, overrides);
  mkdirSync(dirname(resolve(output)), { recursive: true });
  writeFileSync(output, text);
  console.log(`Generated complete notices for ${packages.length} Windows Rust dependencies.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [, , output, overrideFile] = process.argv;
    if (!output) throw new Error('Usage: node scripts/generate-notices.mjs OUTPUT [EXACT_VERSION_OVERRIDES.json]');
    generate(output, overrideFile ?? (existsSync(join(root, 'scripts/license-overrides.json')) ? join(root, 'scripts/license-overrides.json') : undefined));
  } catch (error) {
    console.error(error.status !== undefined ? 'Cargo metadata failed; restore the locked dependencies before generating notices.' : error.message);
    process.exitCode = 1;
  }
}
