import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

/**
 * Standalone web mode config (npm run web:dev / web:build).
 * Uses the same renderer source but runs against the standalone WS server.
 */
export default defineConfig({
  root: 'src/renderer',
  plugins: [svelte()],
  build: {
    outDir: '../../dist',
    emptyOutDir: true,
  },
  server: {
    proxy: {
      // Legacy telepty daemon (HTTP + WS)
      '/api': {
        target: 'http://localhost:3848',
        changeOrigin: true,
        ws: true,
      },
      // aterm WebSocket server
      '/aterm-ws': {
        target: 'ws://localhost:3849',
        changeOrigin: true,
        ws: true,
        rewrite: (path) => path.replace(/^\/aterm-ws/, ''),
      },
    },
  },
})
