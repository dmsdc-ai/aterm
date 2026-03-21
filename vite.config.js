import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// https://vite.dev/config/
export default defineConfig({
  plugins: [svelte()],
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
        // Rewrite the path: /aterm-ws → / (ws server listens at root)
        rewrite: (path) => path.replace(/^\/aterm-ws/, ''),
      },
    },
  },
})
