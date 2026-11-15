import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://crosshair-rs-clean.deep-coati-2988.chatgpt.site',
  output: 'static',
  build: { format: 'file' },
  devToolbar: { enabled: false },
});
