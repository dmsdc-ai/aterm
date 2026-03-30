#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  ensureUserLayout,
  resolveAigentryConfig,
} from '../lib/aigentry.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const packageRoot = path.resolve(__dirname, '..');
const appRoot = path.join(packageRoot, 'dist', 'aterm.app');
const executable = path.join(appRoot, 'Contents', 'MacOS', 'aterm');
const frameworksDir = path.join(appRoot, 'Contents', 'Frameworks');
const packageJson = JSON.parse(
  fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
);

if (process.argv.includes('--version') || process.argv.includes('-v')) {
  console.log(packageJson.version);
  process.exit(0);
}

ensureUserLayout({ version: packageJson.version });

if (!fs.existsSync(executable)) {
  console.error('[aterm] native app bundle is not installed');
  console.error('[aterm] reinstall @dmsdc-ai/aterm on a supported platform');
  process.exit(1);
}

const resolvedConfig = resolveAigentryConfig({ cwd: process.cwd() });
for (const warning of resolvedConfig.warnings) {
  console.warn(warning);
}

const env = { ...process.env };
env.DYLD_LIBRARY_PATH = [frameworksDir, env.DYLD_LIBRARY_PATH]
  .filter(Boolean)
  .join(':');
env.AIGENTRY_CONFIG_JSON = JSON.stringify(resolvedConfig.config);
env.AIGENTRY_CONFIG_LAYERS = JSON.stringify(resolvedConfig.layers);
env.AIGENTRY_SYSTEM_ROOT = resolvedConfig.systemRoot;
env.AIGENTRY_USER_ROOT = resolvedConfig.userRoot;
env.AIGENTRY_PROJECT_ROOT = resolvedConfig.projectRoot;
env.AIGENTRY_PROJECT_AIGENTRY_ROOT = resolvedConfig.projectAigentryRoot;

const child = spawn(executable, process.argv.slice(2), {
  stdio: 'inherit',
  env,
});

child.on('error', error => {
  console.error(`[aterm] failed to launch native bundle: ${error.message}`);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 0);
});
