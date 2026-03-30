#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  ensureUserLayout,
  getUserAtermConfigPath,
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

function printVersionAndExit() {
  console.log(packageJson.version);
  process.exit(0);
}

function readUserSetupState() {
  const configPath = getUserAtermConfigPath();
  if (!fs.existsSync(configPath)) {
    return {
      configPath,
      setupCompleted: false,
    };
  }

  try {
    const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
    return {
      configPath,
      setupCompleted: config.setupCompleted === true,
    };
  } catch (error) {
    console.warn(`[aterm] invalid user config, rerunning setup: ${error.message}`);
    return {
      configPath,
      setupCompleted: false,
    };
  }
}

async function maybeRunFirstRunWizard() {
  ensureUserLayout({ version: packageJson.version });
  const setupState = readUserSetupState();
  if (setupState.setupCompleted) {
    return;
  }

  let wizardModule;
  try {
    wizardModule = await import('../scripts/tui-installer.js');
  } catch (error) {
    console.warn(`[aterm] setup wizard unavailable, launching with defaults (${error.message})`);
    return;
  }

  if (!wizardModule.shouldRunFirstRunWizard()) {
    return;
  }

  const context = wizardModule.buildFirstRunWizardContext(packageJson.version, process.cwd());
  const wizardResult = await wizardModule.runFirstRunWizard(context);
  if (!wizardResult) {
    process.exit(0);
  }

  await wizardModule.completeFirstRunWizard(context, wizardResult);
}

if (process.argv.includes('--version') || process.argv.includes('-v')) {
  printVersionAndExit();
}

await maybeRunFirstRunWizard();
const resolvedConfig = resolveAigentryConfig({ cwd: process.cwd() });

if (!fs.existsSync(executable)) {
  console.error('[aterm] native app bundle is not installed');
  console.error('[aterm] reinstall @dmsdc-ai/aterm on a supported platform');
  process.exit(1);
}

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
