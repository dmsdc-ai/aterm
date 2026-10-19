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

// Executed only by the disposable Node child in the tests below.
// Keep this literal visible for exact-source security review.
const detectCliChildSource = String.raw`
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { pathToFileURL } from 'node:url';

const helperPath = process.argv[1];
const statuses = JSON.parse(process.argv[2]);
const expected = JSON.parse(process.argv[3]);
const commands = ['claude', 'codex', 'gemini'];
assert.equal(statuses.length, commands.length);
const calls = [];
childProcess.spawnSync = (file, args, options) => {
  // No real spawn fallback, including on unexpected or extra calls.
  assert.ok(calls.length < commands.length, 'unexpected extra process invocation');
  const command = commands[calls.length];
  assert.equal(file, '/bin/sh');
  assert.equal(args.length, 2);
  assert.ok(args[0] === '-c' || args[0] === '-lc', 'unexpected shell option');
  assert.equal(args[1], 'command -v ' + command + ' >/dev/null 2>&1');
  assert.deepEqual(options, { stdio: 'ignore' });
  calls.push({ file, args: [...args], options: { ...options } });
  return { status: statuses[calls.length - 1] };
};
syncBuiltinESMExports();
const { detectAiCliStatus } = await import(pathToFileURL(helperPath).href);
const actual = detectAiCliStatus(process.env.HOME);
assert.deepEqual(actual, expected, 'exit statuses must preserve Boolean availability');
assert.equal(calls.length, 3, 'capture must cover every CLI lookup');
assert.deepEqual(calls, commands.map((command) => ({
  file: '/bin/sh',
  args: ['-c', 'command -v ' + command + ' >/dev/null 2>&1'],
  options: { stdio: 'ignore' },
})), 'CLI detection must use non-login shells');
process.stdout.write(JSON.stringify({ actual, captured: calls.length }));
`;

function assertCliDetection(statuses, expected) {
  const root = fs.mkdtempSync(path.join(tmp, 'cli-detection-'));
  const home = path.join(root, 'home');
  const env = {
    HOME: home,
    USERPROFILE: home,
    XDG_CONFIG_HOME: path.join(root, 'config'),
    XDG_DATA_HOME: path.join(root, 'data'),
    TMPDIR: path.join(root, 'tmp'),
    PATH: path.join(root, 'bin'),
    AIGENTRY_SYSTEM_ROOT: path.join(root, 'system'),
  };
  try {
    for (const directory of Object.values(env)) {
      fs.mkdirSync(directory, { recursive: true });
    }
    for (const command of ['claude', 'codex', 'gemini']) {
      fs.mkdirSync(path.join(home, '.' + command));
    }
    const result = spawnSync(process.execPath, [
      '--input-type=module', '--eval', detectCliChildSource,
      path.join(packageRoot, 'lib', 'aigentry.js'),
      JSON.stringify(statuses), JSON.stringify(expected),
    ], {
      cwd: root,
      env,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      timeout: 10_000,
      maxBuffer: 64 * 1024,
      killSignal: 'SIGKILL',
    });
    assert.equal(result.error, undefined, 'bounded Node child must finish');
    assert.equal(result.signal, null);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stderr, '');
    assert.deepEqual(JSON.parse(result.stdout), { actual: expected, captured: 3 });
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

test('CLI detection uses non-login shells with mixed available statuses', () => {
  assertCliDetection([0, 1, 0], { claude: true, codex: false, gemini: true });
});

test('CLI detection preserves complementary unavailable statuses without login shells', () => {
  assertCliDetection([1, 0, 1], { claude: false, codex: true, gemini: false });
});
