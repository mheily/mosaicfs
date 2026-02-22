import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
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
