#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..');
const packageDirName = process.argv[2];

if (!packageDirName) {
  console.error('[aterm npm] usage: node prepare-platform-package.mjs <package-dir>');
  process.exit(1);
}

const packageDir = path.join(__dirname, packageDirName);
const sourceApp = path.join(repoRoot, 'build', 'aterm.app');
const targetApp = path.join(packageDir, 'dist', 'aterm.app');

const build = spawnSync('make', ['app'], {
  cwd: repoRoot,
  stdio: 'inherit',
  env: process.env,
});

if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

if (!fs.existsSync(sourceApp)) {
  console.error(`[aterm npm] missing built app bundle: ${sourceApp}`);
  process.exit(1);
}

fs.rmSync(path.dirname(targetApp), { recursive: true, force: true });
fs.mkdirSync(path.dirname(targetApp), { recursive: true });
fs.cpSync(sourceApp, targetApp, { recursive: true });

console.log(`[aterm npm] staged ${sourceApp} -> ${targetApp}`);
