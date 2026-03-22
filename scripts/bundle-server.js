/**
 * Bundle the Node.js server for Tauri production builds.
 *
 * - Bundles all JS dependencies (ws, etc.) into a single file
 * - Marks node-pty as external (native module — copied separately via tauri resources)
 * - Output: src-tauri/server-bundle.mjs
 */
import { build } from 'esbuild';
import { cpSync, existsSync, mkdirSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(__dirname, '..');

// Step 1: Bundle server JS (everything except node-pty)
await build({
  entryPoints: [resolve(projectRoot, 'src/server/index.js')],
  bundle: true,
  platform: 'node',
  format: 'esm',
  outfile: resolve(projectRoot, 'src-tauri/server-bundle.mjs'),
  external: ['node-pty'],
  banner: {
    js: "import { createRequire } from 'module'; const require = createRequire(import.meta.url);",
  },
});

console.log('[bundle-server] server-bundle.mjs created');

// Step 2: Copy node-pty into src-tauri/node_modules/node-pty
// so tauri can bundle it as a resource
const srcPty = resolve(projectRoot, 'node_modules/node-pty');
const destPty = resolve(projectRoot, 'src-tauri/node_modules/node-pty');

if (existsSync(srcPty)) {
  mkdirSync(dirname(destPty), { recursive: true });
  cpSync(srcPty, destPty, { recursive: true });
  console.log('[bundle-server] node-pty copied to src-tauri/node_modules/node-pty');
} else {
  console.error('[bundle-server] ERROR: node_modules/node-pty not found');
  process.exit(1);
}
