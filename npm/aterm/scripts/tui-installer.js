import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { detectAiCliStatus, resolveInstallHomeDir } from '../lib/aigentry.js';

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

  if (context.mode === 'local') {
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
      detected_clis: context.cliStatus,
    },
  };
}

export async function runInstallerTui(context) {
  const blessed = require('blessed');
  const prompts = (await import('prompts')).default;

  await showInfoScreen(
    blessed,
    ' aterm Installer ',
    [
      '{center}{bold}Aigentry Terminal Installer{/bold}{/center}',
      '',
      'This installer will prepare the requested aigentry config level(s),',
      'write default settings, and stage the native aterm bundle.',
      '',
      `Detected install mode: ${context.mode}`,
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
    installLevels: context.availableLevels.map(level => level.value),
    defaultShell: 'zsh',
    defaultWorkspace: workspaceChoices[0]?.value ?? 'home',
    tailscaleConnect: false,
  };

  const responses = await prompts(
    [
      {
        type: 'multiselect',
        name: 'installLevels',
        message: 'Install level selection',
        choices: context.availableLevels,
        initial: 0,
        instructions: false,
        min: 1,
      },
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
        type: 'toggle',
        name: 'tailscaleConnect',
        message: 'Enable Tailscale connect on launch?',
        initial: false,
        active: 'yes',
        inactive: 'no',
      },
    ],
    {
      onCancel: () => true,
    },
  );

  const mergedAnswers = {
    ...defaultResponses,
    ...responses,
  };

  return {
    installLevels: mergedAnswers.installLevels,
    configPatch: buildConfigPatch(context, mergedAnswers),
    summary: mergedAnswers,
  };
}

export function shouldRunInstallerTui() {
  if (process.env.CI) {
    return false;
  }
  if (process.env.npm_config_yes === 'true') {
    return false;
  }
  return Boolean(process.stdin.isTTY && process.stdout.isTTY);
}

export function buildInstallerContext(mode, projectRoot = null) {
  const installHome = resolveInstallHomeDir();
  const orchestratorPath = path.join(installHome, 'projects', 'aigentry-orchestrator');
  const cliStatus = detectAiCliStatus(installHome);
  const availableLevels = mode === 'global'
    ? [
        {
          title: 'system',
          value: 'system',
          description: 'Create /etc/aigentry/config/aterm.json when writable',
        },
        {
          title: 'user',
          value: 'user',
          description: `Create ${path.join(installHome, '.aigentry')}`,
        },
      ]
    : [
        {
          title: 'project',
          value: 'project',
          description: `Create ${path.join(projectRoot ?? process.cwd(), '.aigentry')}`,
        },
      ];

  return {
    mode,
    projectRoot,
    cliStatus,
    orchestratorPath,
    orchestratorAvailable: require('node:fs').existsSync(orchestratorPath),
    availableLevels,
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
