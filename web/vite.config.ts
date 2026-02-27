import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  optimizeDeps: {
    include: ['pouchdb', 'pouchdb-find', 'cookie'],
  },
  resolve: {
    alias: {
      'events': path.resolve(__dirname, 'node_modules/events/events.js'),
      'pouchdb': path.resolve(__dirname, 'node_modules/pouchdb/lib/index-browser.es.js'),
      'pouchdb-find': path.resolve(__dirname, 'node_modules/pouchdb-find/lib/index-browser.es.js'),
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
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
