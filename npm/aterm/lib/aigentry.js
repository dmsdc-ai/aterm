import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const SYSTEM_ROOT = process.env.AIGENTRY_SYSTEM_ROOT || '/etc/aigentry';

const TASK_QUEUE_DEFAULT = {
  main_topics: [],
  tasks: [],
  completed: [],
};

const LESSONS_DEFAULT = {
  _meta: { version: 1 },
  aterm: {
    invariants: [],
    failed: [],
  },
};

const PROJECT_AGENTS_TEMPLATE = `# Project AI Instructions

- Add project-specific aigentry instructions here.
- This file is project-scoped and overrides user/system defaults when present.
`;

function cloneJsonValue(value) {
  return JSON.parse(JSON.stringify(value));
}

function isPlainObject(value) {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function mergeJson(base, override) {
  if (!isPlainObject(base)) {
    return cloneJsonValue(override);
  }

  const merged = { ...cloneJsonValue(base) };
  for (const [key, value] of Object.entries(override)) {
    if (isPlainObject(value) && isPlainObject(merged[key])) {
      merged[key] = mergeJson(merged[key], value);
      continue;
    }
    merged[key] = cloneJsonValue(value);
  }
  return merged;
}

function ensureDirectory(directoryPath) {
  const existed = fs.existsSync(directoryPath);
  fs.mkdirSync(directoryPath, { recursive: true });
  return !existed;
}

function ensureJsonFile(filePath, defaultValue) {
  ensureDirectory(path.dirname(filePath));
  if (fs.existsSync(filePath)) {
    return false;
  }

  fs.writeFileSync(filePath, `${JSON.stringify(defaultValue, null, 2)}\n`);
  return true;
}

function ensureTextFile(filePath, contents) {
  ensureDirectory(path.dirname(filePath));
  if (fs.existsSync(filePath)) {
    return false;
  }

  fs.writeFileSync(filePath, contents);
  return true;
}

function readJsonFile(filePath, warnings) {
  if (!fs.existsSync(filePath)) {
    return null;
  }

  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (error) {
    warnings.push(`[aterm] invalid JSON config skipped: ${filePath} (${error.message})`);
    return null;
  }
}

function resolveUserHomeFromSudoUser() {
  const sudoUser = process.env.SUDO_USER;
  if (!sudoUser || sudoUser === 'root' || !/^[A-Za-z0-9_.-]+$/.test(sudoUser)) {
    return null;
  }

  const result = spawnSync('/bin/sh', ['-lc', `printf %s ~${sudoUser}`], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  if (result.status !== 0) {
    return null;
  }

  const resolved = result.stdout.trim();
  if (!resolved || resolved === `~${sudoUser}`) {
    return null;
  }

  return resolved;
}

function commandAvailable(command) {
  const result = spawnSync('/bin/sh', ['-lc', `command -v ${command} >/dev/null 2>&1`], {
    stdio: 'ignore',
  });
  return result.status === 0;
}

function findGitRoot(startDir) {
  const resolvedStartDir = path.resolve(startDir);
  const result = spawnSync('git', ['rev-parse', '--show-toplevel'], {
    cwd: resolvedStartDir,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  });

  if (result.status !== 0) {
    return null;
  }

  const gitRoot = result.stdout.trim();
  return gitRoot ? path.resolve(gitRoot) : null;
}

function buildLevelPaths(aigentryRoot, options = {}) {
  const paths = {
    root: aigentryRoot,
    config: path.join(aigentryRoot, 'config'),
    data: path.join(aigentryRoot, 'data'),
    brain_local: path.join(aigentryRoot, 'brain', 'local'),
    brain_profiles: path.join(aigentryRoot, 'brain', 'profiles'),
    inbox: path.join(aigentryRoot, 'inbox'),
    telepty_shared: path.join(aigentryRoot, 'telepty', 'shared'),
    cache_search: path.join(aigentryRoot, 'cache', 'search'),
    logs: path.join(aigentryRoot, 'logs'),
  };

  if (options.legacyTeleptyShared) {
    paths.legacy_telepty_shared = options.legacyTeleptyShared;
  }

  return paths;
}

function buildDefaultConfig(options) {
  const config = {
    version: options.version,
    level: options.level,
    installed_at: new Date().toISOString(),
    platform: `${os.platform()}-${os.arch()}`,
    paths: buildLevelPaths(options.aigentryRoot, {
      legacyTeleptyShared: options.legacyTeleptyShared,
    }),
    shell: {
      default: options.defaultShell ?? 'zsh',
    },
    workspace: {
      default: options.defaultWorkspace ?? 'home',
    },
    orchestrator: {
      enabled: false,
      path: path.join(os.homedir(), 'projects', 'aigentry-orchestrator'),
    },
    tailscale: {
      connect_on_launch: false,
    },
  };

  if (options.projectRoot) {
    config.project_root = options.projectRoot;
  }

  return config;
}

function ensureStructuredRoot(aigentryRoot, options) {
  const reportProgress = options.onProgress ?? (() => {});
  const directories = [
    path.join(aigentryRoot, 'config'),
    path.join(aigentryRoot, 'data'),
    path.join(aigentryRoot, 'brain', 'local'),
    path.join(aigentryRoot, 'brain', 'profiles'),
    path.join(aigentryRoot, 'inbox'),
    path.join(aigentryRoot, 'telepty', 'shared'),
    path.join(aigentryRoot, 'cache', 'search'),
    path.join(aigentryRoot, 'logs'),
  ];

  for (const directoryPath of directories) {
    ensureDirectory(directoryPath);
    reportProgress({ type: 'dir', path: directoryPath });
  }

  if (options.legacyTeleptyShared) {
    ensureDirectory(options.legacyTeleptyShared);
    reportProgress({ type: 'dir', path: options.legacyTeleptyShared });
  }

  const configPath = path.join(aigentryRoot, 'config', 'aterm.json');
  const taskQueuePath = path.join(aigentryRoot, 'data', 'task-queue.json');
  const lessonsPath = path.join(aigentryRoot, 'data', 'lessons.json');
  ensureJsonFile(configPath, buildDefaultConfig(options));
  reportProgress({ type: 'file', path: configPath });
  ensureJsonFile(taskQueuePath, TASK_QUEUE_DEFAULT);
  reportProgress({ type: 'file', path: taskQueuePath });
  ensureJsonFile(lessonsPath, LESSONS_DEFAULT);
  reportProgress({ type: 'file', path: lessonsPath });

  if (options.createAgentsFile) {
    const agentsPath = path.join(aigentryRoot, 'AGENTS.md');
    ensureTextFile(agentsPath, PROJECT_AGENTS_TEMPLATE);
    reportProgress({ type: 'file', path: agentsPath });
  }

  if (options.configPatch) {
    updateConfigFile(configPath, options.configPatch);
    reportProgress({ type: 'file', path: configPath, patched: true });
  }

  return {
    aigentryRoot,
    configPath,
  };
}

export function getSystemAigentryRoot() {
  return SYSTEM_ROOT;
}

export function getUserAigentryRoot(homeDir = os.homedir()) {
  return path.join(homeDir, '.aigentry');
}

export function getLegacyTeleptyShared(homeDir = os.homedir()) {
  return path.join(homeDir, '.telepty', 'shared');
}

export function resolveInstallHomeDir() {
  return resolveUserHomeFromSudoUser() ?? os.homedir();
}

export function resolveProjectRoot(startDir = process.cwd()) {
  return findGitRoot(startDir) ?? path.resolve(startDir);
}

export function isGlobalInstall(env = process.env) {
  return env.npm_config_global === 'true';
}

export function detectAiCliStatus(homeDir = os.homedir()) {
  return {
    claude: commandAvailable('claude') && fs.existsSync(path.join(homeDir, '.claude')),
    codex: commandAvailable('codex') && fs.existsSync(path.join(homeDir, '.codex')),
    gemini: commandAvailable('gemini') && fs.existsSync(path.join(homeDir, '.gemini')),
  };
}

export function updateConfigFile(configPath, patch) {
  const existing = fs.existsSync(configPath)
    ? JSON.parse(fs.readFileSync(configPath, 'utf8'))
    : {};
  const merged = mergeJson(existing, patch);
  fs.writeFileSync(configPath, `${JSON.stringify(merged, null, 2)}\n`);
  return merged;
}

export function ensureSystemLayout(options) {
  const systemRoot = getSystemAigentryRoot();
  const configPath = path.join(systemRoot, 'config', 'aterm.json');

  try {
    ensureDirectory(path.dirname(configPath));
    ensureJsonFile(
      configPath,
      buildDefaultConfig({
        level: 'system',
        version: options.version,
        aigentryRoot: systemRoot,
      }),
    );
    return {
      skipped: false,
      aigentryRoot: systemRoot,
      configPath,
    };
  } catch (error) {
    if (error.code === 'EACCES' || error.code === 'EPERM') {
      return {
        skipped: true,
        reason: `${systemRoot} is not writable`,
      };
    }
    throw error;
  }
}

export function ensureUserLayout(options) {
  const homeDir = options.homeDir ?? resolveInstallHomeDir();
  const userRoot = getUserAigentryRoot(homeDir);
  return ensureStructuredRoot(userRoot, {
    level: 'user',
    version: options.version,
    aigentryRoot: userRoot,
    legacyTeleptyShared: getLegacyTeleptyShared(homeDir),
    defaultShell: options.defaultShell,
    defaultWorkspace: options.defaultWorkspace,
    configPatch: options.configPatch,
    onProgress: options.onProgress,
  });
}

export function addGitignoreSuggestion(projectRoot) {
  const gitRoot = findGitRoot(projectRoot);
  if (!gitRoot) {
    return {
      updated: false,
      gitignorePath: null,
    };
  }

  const gitignorePath = path.join(gitRoot, '.gitignore');
  const entryPattern = /^\s*(?:\/?\.aigentry\/)\s*$/m;
  const currentContents = fs.existsSync(gitignorePath)
    ? fs.readFileSync(gitignorePath, 'utf8')
    : '';

  if (entryPattern.test(currentContents)) {
    return {
      updated: false,
      gitignorePath,
    };
  }

  const prefix = currentContents && !currentContents.endsWith('\n') ? '\n' : '';
  fs.writeFileSync(
    gitignorePath,
    `${currentContents}${prefix}# aterm project config\n.aigentry/\n`,
  );

  return {
    updated: true,
    gitignorePath,
  };
}

export function ensureProjectLayout(options) {
  const projectRoot = resolveProjectRoot(options.startDir);
  const aigentryRoot = path.join(projectRoot, '.aigentry');
  const layout = ensureStructuredRoot(aigentryRoot, {
    level: 'project',
    version: options.version,
    aigentryRoot,
    projectRoot,
    createAgentsFile: true,
    defaultShell: options.defaultShell,
    defaultWorkspace: options.defaultWorkspace,
    configPatch: options.configPatch,
    onProgress: options.onProgress,
  });
  const gitignore = addGitignoreSuggestion(projectRoot);

  return {
    ...layout,
    projectRoot,
    gitignoreUpdated: gitignore.updated,
    gitignorePath: gitignore.gitignorePath,
  };
}

export function resolveAigentryConfig(options = {}) {
  const warnings = [];
  const currentWorkingDirectory = options.cwd ?? process.cwd();
  const userRoot = getUserAigentryRoot();
  const projectRoot = resolveProjectRoot(currentWorkingDirectory);
  const layerCandidates = [
    {
      level: 'system',
      path: path.join(getSystemAigentryRoot(), 'config', 'aterm.json'),
    },
    {
      level: 'user',
      path: path.join(userRoot, 'config', 'aterm.json'),
    },
    {
      level: 'project',
      path: path.join(projectRoot, '.aigentry', 'config', 'aterm.json'),
    },
  ];

  let mergedConfig = {};
  const layers = [];

  for (const layer of layerCandidates) {
    const data = readJsonFile(layer.path, warnings);
    if (!data) {
      continue;
    }
    mergedConfig = mergeJson(mergedConfig, data);
    layers.push({
      level: layer.level,
      path: layer.path,
    });
  }

  return {
    config: mergedConfig,
    layers,
    warnings,
    systemRoot: getSystemAigentryRoot(),
    userRoot,
    projectRoot,
    projectAigentryRoot: path.join(projectRoot, '.aigentry'),
  };
}
