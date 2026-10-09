# aterm

AI-native terminal — a GPU-rendered terminal with first-class session
communication for AI CLIs (Claude, Codex, Gemini). Each tab is a *workspace*
you can drive programmatically: list it, inject text, read its output, and
dispatch tasks to sub-sessions.

> **Status: pre-release (0.2.x).** macOS (Apple Silicon) is the primary
> target; other platform builds are in progress. Interfaces may change
> before 1.0.

## Install

```sh
npm install -g @dmsdc-ai/aterm
```

This installs the `aterm` launcher and CLI; the native binary for your
platform is pulled in as an optional platform package.

## The `aterm` CLI

Inside an aterm workspace the CLI talks to the app over `$ATERM_IPC_SOCKET`.
Outside the app, `aterm list`, `aterm inject` and `aterm status` fall back to
[`telepty`](https://github.com/dmsdc-ai/aigentry-telepty).
Run `aterm` with no arguments to open the app.

| Command | Description |
|---------|-------------|
| `aterm list` | List workspaces |
| `aterm inject <workspace> <text>` | Send text to a workspace |
| `aterm status [<workspace>]` | Show ecosystem / workspace status |
| `aterm create <name> --cli <cli> --cwd <path>` | Create a workspace |
| `aterm dispatch <task-id>` | Decompose → run → collect a task |
| `aterm help` | Full command reference |

## License

MIT — see [LICENSE](./LICENSE).
