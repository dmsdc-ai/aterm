#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  detectAiCliStatus,
  ensureUserLayout,
  getUserAtermConfigPath,
  resolveAigentryConfig,
  resolveInstallHomeDir,
  updateConfigFile,
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

async function maybeRunConfigMigration() {
  const configPath = getUserAtermConfigPath();
  if (!fs.existsSync(configPath)) return;

  let config;
  try {
    config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
  } catch { return; }

  if (!config.setupCompleted) return;

  const isTTY = process.stdin.isTTY && process.stdout.isTTY;
  const cliStatus = detectAiCliStatus(resolveInstallHomeDir());
  const patch = {};
  const questions = [];

  // Check each field individually — users from any version get prompted only for THEIR missing fields
  if (!config.ai?.defaultCLI) {
    questions.push({
      type: 'select',
      name: 'defaultCLI',
      message: 'Default AI CLI for orchestrator workspace',
      choices: [
        { title: 'claude', value: 'claude', disabled: !cliStatus.claude },
        { title: 'codex', value: 'codex', disabled: !cliStatus.codex },
        { title: 'gemini', value: 'gemini', disabled: !cliStatus.gemini },
        { title: 'none (plain zsh)', value: 'none' },
      ],
      initial: cliStatus.claude ? 0 : cliStatus.codex ? 1 : cliStatus.gemini ? 2 : 3,
    });
  }

  if (questions.length === 0) return;

  if (isTTY) {
    try {
      const prompts = (await import('prompts')).default;
      const responses = await prompts(questions);
      if (responses.defaultCLI) {
        patch.ai = { ...config.ai, defaultCLI: responses.defaultCLI };
      }
    } catch { /* fall through to auto-detect */ }
  }

  // Auto-fill any fields not answered via prompt
  if (!patch.ai?.defaultCLI && !config.ai?.defaultCLI) {
    const auto = cliStatus.claude ? 'claude' : cliStatus.codex ? 'codex' : cliStatus.gemini ? 'gemini' : 'none';
    patch.ai = { ...config.ai, ...patch.ai, defaultCLI: auto };
  }

  if (Object.keys(patch).length > 0) {
    updateConfigFile(configPath, patch);
  }
}

if (process.argv.includes('--version') || process.argv.includes('-v')) {
  printVersionAndExit();
}

await maybeRunFirstRunWizard();
await maybeRunConfigMigration();
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
