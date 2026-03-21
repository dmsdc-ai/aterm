import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/telepty': {
        target: 'http://localhost:3849',
        changeOrigin: true,
        ws: true,
      },
      '/api': {
        target: 'http://localhost:3849',
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  // Clear screen disabled for Tauri dev integration
  clearScreen: false,
});
