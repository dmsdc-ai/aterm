#!/usr/bin/env node
import net from 'net';
import path from 'path';
import os from 'os';

const SOCKET_PATH = path.join(os.homedir(), '.aterm', 'aterm.sock');

// ── Argument parsing ──────────────────────────────────────────────────────────

const args = process.argv.slice(2);

function getFlag(flags, defaultValue = null) {
  for (const flag of flags) {
    const idx = args.indexOf(flag);
    if (idx !== -1 && idx + 1 < args.length) return args[idx + 1];
  }
  return defaultValue;
}

function hasFlag(...flags) {
  return flags.some(f => args.includes(f));
}

const command = args[0];

// ── Socket RPC ────────────────────────────────────────────────────────────────

function sendRequest(payload) {
  return new Promise((resolve, reject) => {
    const client = net.connect(SOCKET_PATH, () => {
      client.write(JSON.stringify(payload) + '\n');
    });

    let buffer = '';

    client.on('data', chunk => {
      buffer += chunk.toString();
      const lines = buffer.split('\n');
      buffer = lines.pop();
      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const msg = JSON.parse(line);
          client.destroy();
          resolve(msg);
        } catch {
          client.destroy();
          reject(new Error('Invalid JSON response: ' + line));
        }
      }
    });

    client.on('error', err => {
      if (err.code === 'ENOENT' || err.code === 'ECONNREFUSED') {
        reject(new Error('NOT_RUNNING'));
      } else {
        reject(err);
      }
    });

    client.on('end', () => {
      if (buffer.trim()) {
        try {
          resolve(JSON.parse(buffer));
        } catch {
          reject(new Error('Incomplete response'));
        }
      }
    });
  });
}

// ── Output helpers ────────────────────────────────────────────────────────────

function printTable(rows, columns) {
  if (!rows.length) {
    console.log('(no workspaces)');
    return;
  }
  const widths = columns.map(col =>
    Math.max(col.label.length, ...rows.map(r => String(r[col.key] ?? '').length))
  );
  const header = columns.map((col, i) => col.label.padEnd(widths[i])).join('  ');
  const divider = widths.map(w => '─'.repeat(w)).join('  ');
  console.log(header);
  console.log(divider);
  for (const row of rows) {
    console.log(columns.map((col, i) => String(row[col.key] ?? '').padEnd(widths[i])).join('  '));
  }
}

function formatDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  return d.toLocaleString();
}

// ── Command handlers ──────────────────────────────────────────────────────────

async function cmdSend() {
  const workspaceId = getFlag(['-w', '--workspace']);
  const text = args[args.length - 1];
  if (!workspaceId) { console.error('Error: --workspace / -w required'); process.exit(1); }
  if (!text || text.startsWith('-')) { console.error('Error: text argument required'); process.exit(1); }
  const res = await sendRequest({ cmd: 'send', workspace: workspaceId, text });
  if (res.error) { console.error('Error:', res.error); process.exit(1); }
  console.log('OK');
}

async function cmdSendKey() {
  const workspaceId = getFlag(['-w', '--workspace']);
  const key = args[args.length - 1];
  if (!workspaceId) { console.error('Error: --workspace / -w required'); process.exit(1); }
  if (!key || key.startsWith('-')) { console.error('Error: key argument required'); process.exit(1); }
  const res = await sendRequest({ cmd: 'send-key', workspace: workspaceId, key });
  if (res.error) { console.error('Error:', res.error); process.exit(1); }
  console.log('OK');
}

async function cmdReadScreen() {
  const workspaceId = getFlag(['-w', '--workspace']);
  const lines = getFlag(['--lines']);
  if (!workspaceId) { console.error('Error: --workspace / -w required'); process.exit(1); }
  const payload = { cmd: 'read-screen', workspace: workspaceId };
  if (lines) payload.lines = parseInt(lines, 10);
  const res = await sendRequest(payload);
  if (res.error) { console.error('Error:', res.error); process.exit(1); }
  const content = Array.isArray(res.lines) ? res.lines.join('\n') : (res.content ?? '');
  process.stdout.write(content);
  if (!content.endsWith('\n')) process.stdout.write('\n');
}

async function cmdListWorkspaces() {
  const res = await sendRequest({ cmd: 'list-workspaces' });
  if (res.error) { console.error('Error:', res.error); process.exit(1); }
  const workspaces = (res.workspaces ?? []).map(w => ({ ...w, created: formatDate(w.created) }));
  printTable(workspaces, [
    { key: 'id',      label: 'ID'      },
    { key: 'cwd',     label: 'CWD'     },
    { key: 'status',  label: 'STATUS'  },
    { key: 'created', label: 'CREATED' },
  ]);
}

async function cmdNewWorkspace() {
  const cwd = getFlag(['-c', '--cwd']) ?? process.cwd();
  const cmd = getFlag(['--command']);
  const payload = { cmd: 'new-workspace', cwd };
  if (cmd) payload.command = cmd;
  const res = await sendRequest(payload);
  if (res.error) { console.error('Error:', res.error); process.exit(1); }
  console.log(res.workspaceId ?? res.id ?? 'OK');
}

async function cmdCloseWorkspace() {
  const workspaceId = getFlag(['-w', '--workspace']);
  if (!workspaceId) { console.error('Error: --workspace / -w required'); process.exit(1); }
  const res = await sendRequest({ cmd: 'close-workspace', workspace: workspaceId });
  if (res.error) { console.error('Error:', res.error); process.exit(1); }
  console.log('OK');
}

async function cmdStatus() {
  const res = await sendRequest({ cmd: 'status' });
  if (res.error) { console.error('Error:', res.error); process.exit(1); }
  const count = res.workspaces ?? res.workspaceCount ?? res.count ?? 0;
  const uptime = res.uptime != null ? ` | uptime: ${res.uptime}s` : '';
  const version = res.version ? ` | version: ${res.version}` : '';
  console.log(`aterm running${version} | workspaces: ${count}${uptime}`);
}

function printHelp() {
  console.log(`aterm — AI terminal workspace CLI

USAGE:
  aterm <command> [options]

COMMANDS:
  send           -w <id> "text"          Send text to workspace
  send-key       -w <id> <key>           Send key (e.g. return, ctrl+c)
  read-screen    -w <id> [--lines N]     Read terminal screen content
  list-workspaces / ls                   List all workspaces
  new-workspace  [-c <cwd>] [--command]  Create new workspace
  new            (alias for new-workspace)
  close-workspace -w <id>               Close a workspace
  close          (alias for close-workspace)
  status                                 Server status
  help                                   Show this help

EXAMPLES:
  aterm send -w abc123 "ls -la"
  aterm send-key -w abc123 return
  aterm read-screen -w abc123 --lines 20
  aterm ls
  aterm new -c /tmp --command bash
  aterm close -w abc123
  aterm status
`);
}

// ── Main ──────────────────────────────────────────────────────────────────────

async function main() {
  if (!command || command === 'help' || hasFlag('-h', '--help')) {
    printHelp();
    return;
  }

  try {
    switch (command) {
      case 'send':            await cmdSend(); break;
      case 'send-key':        await cmdSendKey(); break;
      case 'read-screen':     await cmdReadScreen(); break;
      case 'list-workspaces':
      case 'ls':              await cmdListWorkspaces(); break;
      case 'new-workspace':
      case 'new':             await cmdNewWorkspace(); break;
      case 'close-workspace':
      case 'close':           await cmdCloseWorkspace(); break;
      case 'status':          await cmdStatus(); break;
      default:
        console.error(`Unknown command: ${command}`);
        console.error('Run "aterm help" for usage.');
        process.exit(1);
    }
  } catch (err) {
    if (err.message === 'NOT_RUNNING') {
      console.error('aterm server not running. Start with: node src/server/index.js');
    } else {
      console.error('Error:', err.message);
    }
    process.exit(1);
  }
}

main();
