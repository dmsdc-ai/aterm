import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const packageRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const packageJson = JSON.parse(
  fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
);

let tmp;

function writeExecutable(file, body) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, body, { mode: 0o755 });
}

function runLauncher(args) {
  // HOME points at the tmp dir and SUDO_USER is dropped so ensureUserLayout
  // could only ever write under tmp.
  const env = { ...process.env, HOME: tmp };
  delete env.SUDO_USER;
  return spawnSync(process.execPath, [path.join(tmp, 'bin', 'aterm.js'), ...args], {
    cwd: tmp,
    env,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
}

before(() => {
  tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'aterm-launcher-')));
  fs.cpSync(path.join(packageRoot, 'bin'), path.join(tmp, 'bin'), { recursive: true });
  fs.cpSync(path.join(packageRoot, 'lib'), path.join(tmp, 'lib'), { recursive: true });
  fs.copyFileSync(path.join(packageRoot, 'package.json'), path.join(tmp, 'package.json'));

  const contents = path.join(tmp, 'dist', 'aterm.app', 'Contents');
  writeExecutable(
    path.join(contents, 'Resources', 'bin', 'aterm'),
    `#!/bin/sh\nprintf '%s\\n' "$@" > '${path.join(tmp, 'cli-args')}'\nexit 7\n`,
  );
  writeExecutable(
    path.join(contents, 'MacOS', 'aterm'),
    `#!/bin/sh\ntouch '${path.join(tmp, 'launched')}'\n`,
  );
});

after(() => {
  if (tmp) fs.rmSync(tmp, { recursive: true, force: true });
});

test('subcommand runs the bundled CLI, not the app', () => {
  const r = runLauncher(['list', '--json']);
  assert.equal(r.status, 7, `stderr: ${r.stderr}`);
  const recorded = fs.readFileSync(path.join(tmp, 'cli-args'), 'utf8');
  assert.deepEqual(recorded.trimEnd().split('\n'), ['list', '--json']);
  assert.equal(fs.existsSync(path.join(tmp, 'launched')), false);
  assert.equal(fs.existsSync(path.join(tmp, '.aigentry')), false);
});

test('--version still prints the package version', () => {
  const r = runLauncher(['--version']);
  assert.equal(r.status, 0, `stderr: ${r.stderr}`);
  assert.equal(r.stdout.trim(), packageJson.version);
});
