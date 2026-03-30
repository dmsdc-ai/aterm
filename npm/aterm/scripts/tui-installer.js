import path from 'node:path';
import { createRequire } from 'node:module';
import {
  detectAiCliStatus,
  resolveInstallHomeDir,
  resolveProjectRoot,
  updateConfigFile,
} from '../lib/aigentry.js';

const require = createRequire(import.meta.url);

function waitForKeypress(screen, keys = ['enter', 'space', 'escape']) {
  return new Promise((resolve) => {
    const handler = (_, key) => {
      if (!key || !keys.includes(key.name)) {
        return;
      }
      screen.off('keypress', handler);
      resolve();
    };
    screen.on('keypress', handler);
  });
}

async function showInfoScreen(blessed, title, lines, footer) {
  const screen = blessed.screen({
    smartCSR: true,
    title: 'aterm installer',
    fullUnicode: true,
  });

  blessed.box({
    parent: screen,
    top: 'center',
    left: 'center',
    width: '78%',
    height: Math.max(12, lines.length + 6),
    border: { type: 'line' },
    tags: true,
    label: ` ${title} `,
    style: {
      border: { fg: 'cyan' },
      fg: 'white',
    },
    content: `${lines.join('\n')}\n\n${footer}`,
  });

  screen.render();
  await waitForKeypress(screen);
  screen.destroy();
  // Reset terminal modes that blessed may leave set (DECCKM, keypad)
  // so that subsequent prompts receive normal arrow key sequences.
  process.stdout.write('\x1b[?1l\x1b>');
}

function buildWorkspaceChoices(context) {
  const choices = [];
  if (context.orchestratorAvailable) {
    choices.push({
      title: 'aigentry-orchestrator',
      value: 'orchestrator',
      description: context.orchestratorPath,
    });
  }

  if (context.projectRoot) {
    choices.push({
      title: 'current project',
      value: 'project',
      description: context.projectRoot,
    });
  }

  choices.push({
    title: 'home directory',
    value: 'home',
    description: resolveInstallHomeDir(),
  });

  return choices;
}

function buildConfigPatch(context, answers) {
  const workspacePathMap = {
    orchestrator: context.orchestratorPath,
    project: context.projectRoot,
    home: resolveInstallHomeDir(),
  };

  return {
    shell: {
      default: answers.defaultShell,
    },
    workspace: {
      default: answers.defaultWorkspace,
      path: workspacePathMap[answers.defaultWorkspace] ?? resolveInstallHomeDir(),
    },
    orchestrator: {
      enabled: answers.defaultWorkspace === 'orchestrator',
      path: context.orchestratorPath,
    },
    tailscale: {
      connect_on_launch: answers.tailscaleConnect,
    },
    ai: {
      defaultCLI: answers.defaultCLI,
      detected_clis: context.cliStatus,
    },
  };
}

export async function runFirstRunWizard(context) {
  const blessed = require('blessed');
  const prompts = (await import('prompts')).default;

  await showInfoScreen(
    blessed,
    ' aterm Installer ',
    [
      '{center}{bold}Aigentry Terminal Installer{/bold}{/center}',
      '',
      'This wizard finishes your aterm setup before the native app launches.',
      'It will save your preferred shell, workspace, and Tailscale defaults.',
      '',
      `Package version: ${context.version}`,
    ],
    '{gray-fg}Press Enter to continue{/}',
  );

  const cliLines = [
    '{bold}AI CLI detection{/bold}',
    '',
    `claude ${context.cliStatus.claude ? '✅' : '❌'}`,
    `codex  ${context.cliStatus.codex ? '✅' : '❌'}`,
    `gemini ${context.cliStatus.gemini ? '✅' : '❌'}`,
  ];

  await showInfoScreen(
    blessed,
    ' CLI Status ',
    cliLines,
    '{gray-fg}Press Enter to continue{/}',
  );

  const workspaceChoices = buildWorkspaceChoices(context);
  const defaultResponses = {
    defaultShell: 'zsh',
    defaultWorkspace: workspaceChoices[0]?.value ?? 'home',
    defaultCLI: context.cliStatus.claude ? 'claude' : context.cliStatus.codex ? 'codex' : context.cliStatus.gemini ? 'gemini' : 'none',
    tailscaleConnect: false,
  };
  let cancelled = false;

  const responses = await prompts(
    [
      {
        type: 'select',
        name: 'defaultShell',
        message: 'Default shell',
        choices: [
          { title: 'zsh', value: 'zsh' },
          { title: 'bash', value: 'bash' },
          { title: 'fish', value: 'fish' },
        ],
        initial: 0,
      },
      {
        type: 'select',
        name: 'defaultWorkspace',
        message: 'Default workspace / orchestrator target',
        choices: workspaceChoices,
        initial: 0,
      },
      {
        type: 'select',
        name: 'defaultCLI',
        message: 'Default AI CLI for orchestrator workspace',
        choices: [
          { title: 'claude', value: 'claude', disabled: !context.cliStatus.claude },
          { title: 'codex', value: 'codex', disabled: !context.cliStatus.codex },
          { title: 'gemini', value: 'gemini', disabled: !context.cliStatus.gemini },
          { title: 'none (plain zsh)', value: 'none' },
        ],
        initial: context.cliStatus.claude ? 0 : context.cliStatus.codex ? 1 : context.cliStatus.gemini ? 2 : 3,
      },
      {
        type: 'toggle',
        name: 'tailscaleConnect',
        message: 'Enable Tailscale connect on launch?',
        initial: false,
        active: 'yes',
        inactive: 'no',
      },
    ],
    {
      onCancel: () => {
        cancelled = true;
        return false;
      },
    },
  );

  if (cancelled) {
    return null;
  }

  const mergedAnswers = {
    ...defaultResponses,
    ...responses,
  };

  return {
    configPatch: buildConfigPatch(context, mergedAnswers),
    summary: mergedAnswers,
  };
}

export function shouldRunFirstRunWizard() {
  if (process.env.CI) {
    return false;
  }
  if (process.env.npm_config_yes === 'true') {
    return false;
  }
  return Boolean(process.stdin.isTTY && process.stdout.isTTY);
}

export function buildFirstRunWizardContext(version, cwd = process.cwd()) {
  const installHome = resolveInstallHomeDir();
  const projectRoot = resolveProjectRoot(cwd);
  const orchestratorPath = path.join(installHome, 'projects', 'aigentry-orchestrator');
  const cliStatus = detectAiCliStatus(installHome);

  return {
    version,
    projectRoot,
    cliStatus,
    orchestratorPath,
    orchestratorAvailable: require('node:fs').existsSync(orchestratorPath),
    userConfigPath: path.join(installHome, '.aigentry', 'config', 'aterm.json'),
  };
}

export async function showProgressScreen(runSteps) {
  const blessed = require('blessed');
  const screen = blessed.screen({
    smartCSR: true,
    title: 'aterm installer',
    fullUnicode: true,
  });

  const box = blessed.log({
    parent: screen,
    top: 'center',
    left: 'center',
    width: '84%',
    height: '70%',
    border: { type: 'line' },
    tags: true,
    label: ' Installation Progress ',
    style: {
      border: { fg: 'green' },
      fg: 'white',
    },
    scrollable: true,
    alwaysScroll: true,
    scrollbar: { ch: ' ', inverse: true },
  });

  const report = (line) => {
    box.add(line);
    screen.render();
  };

  screen.render();
  await runSteps(report);
  await new Promise(resolve => setTimeout(resolve, 250));
  screen.destroy();
  process.stdout.write('\x1b[?1l\x1b>');
}

export async function showDoneScreen(summaryLines) {
  const blessed = require('blessed');
  await showInfoScreen(
    blessed,
    ' Install Complete ',
    summaryLines,
    '{green-fg}Run: aterm{/}',
  );
}

export async function completeFirstRunWizard(context, wizardResult) {
  const blessed = require('blessed');
  await showProgressScreen(async (progress) => {
    progress('{bold}Saving setup{/bold}');
    updateConfigFile(context.userConfigPath, {
      ...wizardResult.configPatch,
      setupCompleted: true,
    });
    progress(`{green-fg}ok{/} ${context.userConfigPath}`);
  });

  await showDoneScreen([
    'aterm setup is complete.',
    '',
    `Shell: ${wizardResult.summary.defaultShell}`,
    `AI CLI: ${wizardResult.summary.defaultCLI}`,
    `Workspace: ${wizardResult.summary.defaultWorkspace}`,
    `Tailscale connect: ${wizardResult.summary.tailscaleConnect ? 'yes' : 'no'}`,
  ]);
}
