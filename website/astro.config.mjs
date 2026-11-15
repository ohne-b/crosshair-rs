import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://ohne-b.github.io',
  base: '/crosshair-rs/',
  output: 'static',
  build: { format: 'file' },
  devToolbar: { enabled: false },
});
