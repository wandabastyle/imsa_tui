import { fileURLToPath, URL } from 'node:url';

import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite-plus';

export default defineConfig({
  build: {
    emptyOutDir: true,
    outDir: 'build',
  },
  fmt: {
    ignorePatterns: ['build/**', 'src/lib/generated/**'],
    semi: true,
    singleQuote: true,
    sortImports: {
      enabled: true,
      newlinesBetween: false,
      partitionByNewline: true,
    },
  },
  lint: {
    ignorePatterns: ['build/**', 'src/lib/generated/**'],
    options: {
      typeAware: true,
      typeCheck: true,
    },
  },
  plugins: [react()],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
    },
  },
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8080',
      '/auth': 'http://127.0.0.1:8080',
      '/healthz': 'http://127.0.0.1:8080',
      '/readyz': 'http://127.0.0.1:8080',
    },
  },
});
