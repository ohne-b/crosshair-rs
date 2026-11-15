// Keep all Astro commands telemetry-free on Windows, macOS and Linux.
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';

const require = createRequire(import.meta.url);
const cli = resolve(
  dirname(require.resolve('astro/package.json')),
  'bin/astro.mjs',
);
const child = spawn(process.execPath, [cli, ...process.argv.slice(2)], {
  stdio: 'inherit',
  env: { ...process.env, ASTRO_TELEMETRY_DISABLED: '1' },
});
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal));
}
child.on('error', (error) => {
  console.error(error.message);
  process.exitCode = 1;
});
child.on('exit', (code) => {
  process.exitCode = code ?? 1;
});
