import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

const isTauri = !!process.env.TAURI_ENV_PLATFORM;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  optimizeDeps: {
    include: ['pouchdb', 'pouchdb-find', 'cookie'],
  },
  build: {
    // Tauri uses WebKit on macOS — target Safari 16 for compatibility
    ...(isTauri ? { target: 'safari16' } : {}),
    rollupOptions: {
      // Tauri packages are only available in the desktop runtime.
      // Mark them as external so the web-only build doesn't fail.
      external: isTauri ? [] : [
        /^@tauri-apps\//,
      ],
    },
  },
  resolve: {
    alias: {
      // react-router v7's development bundle imports several server-only CJS
      // packages (cookie, set-cookie-parser) and a Node built-in (async_hooks).
      // In a browser build these code paths are never reached at runtime, but
      // Rollup still tries to resolve the imports. Vite's dep-optimizer normally
      // pre-bundles them, but that step can be skipped in rootless Podman builds.
      // Pin each to its CJS entry so Rollup always finds them; Vite's built-in
      // @rollup/plugin-commonjs converts CJS to ESM automatically.
      'cookie': path.resolve(__dirname, 'node_modules/cookie/dist/index.js'),
      'set-cookie-parser': path.resolve(__dirname, 'node_modules/set-cookie-parser/lib/set-cookie.js'),
      'node:async_hooks': path.resolve(__dirname, 'src/lib/async-hooks-stub.ts'),
      'events': path.resolve(__dirname, 'node_modules/events/events.js'),
      'pouchdb': path.resolve(__dirname, 'node_modules/pouchdb/lib/index-browser.es.js'),
      'pouchdb-find': path.resolve(__dirname, 'node_modules/pouchdb-find/lib/index-browser.es.js'),
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': {
        target: 'https://localhost:8443',
        secure: false,
        changeOrigin: true,
      },
      '/db': {
        target: 'http://localhost:5984',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/db/, ''),
      },
    },
  },
})
