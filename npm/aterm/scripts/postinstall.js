#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import {
  detectAiCliStatus,
  ensureProjectLayout,
  ensureSystemLayout,
  ensureUserLayout,
  isGlobalInstall,
  resolveInstallHomeDir,
  resolveProjectRoot,
} from '../lib/aigentry.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const packageRoot = path.resolve(__dirname, '..');
const require = createRequire(import.meta.url);
const packageJson = JSON.parse(
  fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
);
const TTY_REATTACH_FLAG = 'ATERM_POSTINSTALL_TTY_ATTACHED';

console.log(`[aterm] postinstall start (${packageJson.version})`);

const PLATFORM_PACKAGES = {
  darwin: {
    arm64: '@dmsdc-ai/aterm-darwin-arm64',
  },
};

function resolvePlatformPackage() {
  return PLATFORM_PACKAGES[process.platform]?.[process.arch] ?? null;
}

function canUseInteractiveInstaller() {
  if (process.env.CI) {
    return false;
  }
  if (process.env.npm_config_yes === 'true') {
    return false;
  }
  return true;
}

function maybeReattachTTY() {
  if (!canUseInteractiveInstaller()) {
    return;
  }
  if (process.env[TTY_REATTACH_FLAG] === '1') {
    return;
  }
  if (process.stdin.isTTY && process.stdout.isTTY) {
    return;
  }
  if (process.platform === 'win32') {
    return;
  }

  let ttyIn;
  let ttyOut;
  let ttyErr;

  try {
    ttyIn = fs.openSync('/dev/tty', 'r');
    ttyOut = fs.openSync('/dev/tty', 'w');
    ttyErr = fs.openSync('/dev/tty', 'w');
  } catch {
    return;
  }

  console.log('[aterm] reattaching postinstall to /dev/tty for interactive installer');
  const result = spawnSync(process.execPath, [__filename], {
    cwd: process.cwd(),
    env: {
      ...process.env,
      [TTY_REATTACH_FLAG]: '1',
    },
    stdio: [ttyIn, ttyOut, ttyErr],
  });

  fs.closeSync(ttyIn);
  fs.closeSync(ttyOut);
  fs.closeSync(ttyErr);

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }

  process.exit(0);
}

async function collectInstallerPlan() {
  const mode = isGlobalInstall() ? 'global' : 'local';
  const installRoot = process.env.INIT_CWD ? path.resolve(process.env.INIT_CWD) : process.cwd();
  const projectRoot = mode === 'local' ? resolveProjectRoot(installRoot) : null;

  const defaultPlan = {
    installLevels: mode === 'global' ? ['system', 'user'] : ['project'],
    configPatch: {
      shell: { default: 'zsh' },
      workspace: {
        default: mode === 'local' ? 'project' : 'home',
        path: mode === 'local' ? projectRoot : resolveInstallHomeDir(),
      },
      orchestrator: {
        enabled: false,
        path: path.join(resolveInstallHomeDir(), 'projects', 'aigentry-orchestrator'),
      },
      tailscale: {
        connect_on_launch: false,
      },
      ai: {
        detected_clis: detectAiCliStatus(resolveInstallHomeDir()),
      },
    },
    mode,
    projectRoot,
  };

  let tuiModule;
  try {
    tuiModule = await import('./tui-installer.js');
  } catch (error) {
    console.warn(`[aterm] installer TUI unavailable, falling back to defaults (${error.message})`);
    return defaultPlan;
  }

  if (!tuiModule.shouldRunInstallerTui()) {
    return defaultPlan;
  }

  const context = tuiModule.buildInstallerContext(mode, projectRoot);
  const plan = await tuiModule.runInstallerTui(context);
  return {
    ...defaultPlan,
    ...plan,
  };
}

function applyInstallPlan(plan, progress) {
  const onProgress = (event) => {
    if (!progress) {
      return;
    }
    if (event.type === 'dir') {
      progress(`{green-fg}dir{/} ${event.path}`);
      return;
    }
    progress(`{cyan-fg}file{/} ${event.path}`);
  };

  if (plan.installLevels.includes('system')) {
    progress?.('{bold}Preparing system level{/bold}');
    const systemLayout = ensureSystemLayout({
      version: packageJson.version,
    });
    if (systemLayout.skipped) {
      progress?.(`{yellow-fg}skip{/} system: ${systemLayout.reason}`);
    } else {
      progress?.(`{green-fg}ok{/} ${systemLayout.configPath}`);
    }
  }

  if (plan.installLevels.includes('user')) {
    progress?.('{bold}Preparing user level{/bold}');
    const userLayout = ensureUserLayout({
      version: packageJson.version,
      configPatch: plan.configPatch,
      defaultShell: plan.configPatch.shell?.default,
      defaultWorkspace: plan.configPatch.workspace?.default,
      onProgress,
    });
    progress?.(`{green-fg}ok{/} ${userLayout.configPath}`);
  }

  if (plan.installLevels.includes('project')) {
    progress?.('{bold}Preparing project level{/bold}');
    const projectLayout = ensureProjectLayout({
      version: packageJson.version,
      startDir: plan.projectRoot,
      configPatch: plan.configPatch,
      defaultShell: plan.configPatch.shell?.default,
      defaultWorkspace: plan.configPatch.workspace?.default,
      onProgress,
    });
    progress?.(`{green-fg}ok{/} ${projectLayout.configPath}`);
    if (projectLayout.gitignoreUpdated && projectLayout.gitignorePath) {
      progress?.(`{green-fg}ok{/} added .aigentry/ to ${projectLayout.gitignorePath}`);
    }
  }
}

function installNativeBundle() {
  const platformPackage = resolvePlatformPackage();
  if (!platformPackage) {
    console.warn(
      `[aterm] no native package for ${process.platform} ${process.arch}; install skipped`,
    );
    process.exit(0);
  }

  let platformPackageJsonPath;
  try {
    platformPackageJsonPath = require.resolve(`${platformPackage}/package.json`);
  } catch {
    console.warn(`[aterm] optional native package not installed: ${platformPackage}`);
    console.warn('[aterm] reinstall after the platform package is available');
    process.exit(0);
  }

  const platformRoot = path.dirname(platformPackageJsonPath);
  const sourceApp = path.join(platformRoot, 'dist', 'aterm.app');
  const targetApp = path.join(packageRoot, 'dist', 'aterm.app');

  if (!fs.existsSync(sourceApp)) {
    console.error(`[aterm] native app bundle missing from ${platformPackage}: ${sourceApp}`);
    process.exit(1);
  }

  fs.rmSync(path.dirname(targetApp), { recursive: true, force: true });
  fs.mkdirSync(path.dirname(targetApp), { recursive: true });
  fs.cpSync(sourceApp, targetApp, { recursive: true });

  console.log(`[aterm] installed native bundle from ${platformPackage}`);
}

async function main() {
  maybeReattachTTY();
  const plan = await collectInstallerPlan();

  let tuiModule = null;
  try {
    tuiModule = await import('./tui-installer.js');
  } catch {
    tuiModule = null;
  }

  if (tuiModule?.shouldRunInstallerTui()) {
    await tuiModule.showProgressScreen(async (progress) => {
      applyInstallPlan(plan, progress);
      progress('{bold}Staging native bundle{/bold}');
      installNativeBundle();
      progress('{green-fg}ok{/} native bundle ready');
    });

    await tuiModule.showDoneScreen([
      'aterm setup is complete.',
      '',
      `Mode: ${plan.mode}`,
      `Levels: ${plan.installLevels.join(', ')}`,
      `Shell: ${plan.configPatch.shell?.default ?? 'zsh'}`,
      `Workspace: ${plan.configPatch.workspace?.default ?? 'home'}`,
      `Tailscale connect: ${plan.configPatch.tailscale?.connect_on_launch ? 'yes' : 'no'}`,
    ]);
    return;
  }

  applyInstallPlan(plan);
  installNativeBundle();
}

await main();
