# Changelog

All notable changes to **aterm** are documented in this file.

---

## [0.1.46] — 2026-04-01

Cumulative release covering v0.1.40 through v0.1.46. Major milestones: IPC Phase 1 complete, session persistence moved to Rust FFI, GPU renderer overhauled, Settings UI, devkit integration, and dozens of stability fixes.

### Added

#### IPC Phase 1 (steps 1-7)
- `aterm-session` and `aterm-ipc` crates — session types, actions, IPC server with auth (`019b3c8`)
- `AtermApp` singleton with IPC dispatch for session management (`5b3173b`)
- `aterm_dispatch` + `aterm_set_host` C ABI for cross-language IPC (`5180c19`)
- Swift host callback registration via `aterm_set_host` (`730549a`)
- PTY environment variables (`ATERM_IPC_SOCKET`, `ATERM_WORKSPACE`) and telepty bridge (`7a96d3a`)

#### Session Management CLI (`bin/aterm`)
- `aterm list`, `aterm create`, `aterm kill`, `aterm inject` — full session lifecycle
- `aterm tasks` (list/add/done), `aterm lessons` (list/add) — task board and lessons board
- `aterm dispatch` — autonomous sub-session orchestration with CLI selection and polling
- Natural-language help mapping for all commands
- `ListTasks`, `ListLessons` IPC action variants (`f7b8dca`)

#### GPU Renderer Overhaul
- `wgpu` renderer rewrite with improved glyph pipeline via `glyphon`
- `AtermTheme` system — configurable color themes (Dark, Light, Solarized Dark, Nord, Dracula, Monokai, One Dark, Gruvbox, Tokyo Night)
- Theme-aware rendering across terminal, sidebar, and UI chrome

#### Settings UI
- `SettingsView` (Cmd+,) — font size, font family, line spacing, cursor style, cursor blink, theme picker
- `AtermLocalization` — English / Korean UI language support
- All settings backed by `~/.aigentry/config/aterm.json`

#### Session Persistence (Rust FFI)
- `SessionEntry` and `Workspace` persistence metadata (`f25a69c`)
- Atomic writes and one-time Swift format migration (`bacdc19`)
- FFI bridge for platform UIs (`bd52603`)
- Replaced Swift session persistence with Rust FFI (`da1ffd4`)

#### Devkit Integration
- Delegate AGENTS.md / GEMINI.md generation to `aigentry-devkit workspace-init` (`996dab3`)
- Launcher exposes `node_modules/.bin` in PATH for native app binary discovery
- Pin devkit `^0.0.7`

#### Other
- CLI presets for workspace bootstrap (claude, codex, gemini)
- Telepty bridge enhancements for cross-terminal session communication
- `aterm-bridge.h` — expanded C header for Swift interop

### Fixed

- **Empty session name** — sessions no longer created with blank names (#104) (`4612978`)
- **Inject routing** — route inject through inject queue registry, not empty PtyManager (`682c9c7`)
- **Session restore gate** — gate on `needsOnboarding` instead of `setupCompleted` (`c879a1f`)
- **First-run guard** — clean both old and new session paths (`007b39e`)
- **CLI spawning** — spawn selected CLI directly instead of zsh wrapper (`9948475`)
- **npm packaging** — pin platform package to current version in optionalDependencies (`32e74fd`)
- **Bootstrap command** — pass `bootstrapCommand` to `terminalView.spawnCommand` in `createWorkspace` (`3cbf14d`)

### Changed

- Sidebar UX improvements — better layout, selection state, naming
- Version bump 0.1.39 → 0.1.46

---

## [0.1.39] — 2026-03-31 (and earlier)

Rapid stabilization of onboarding, CLI bootstrap, and shell readiness.

### Fixed

- Onboarding sheet height 520 → 600 to fit all content (v0.1.16)
- Wait for shell prompt before sending bootstrap command (v0.1.17)
- Process-based shell readiness instead of screen patterns (v0.1.18)
- Pre-trust Claude Code workspace by creating project dir (v0.1.19)
- Orchestrator never fails — CLI fallback to plain shell (v0.1.20)
- Restart reuses existing shell + `cliGaveUp` + naming (v0.1.21)
- Onboarding text contrast on dark background (v0.1.22)
- Tailscale DNS spam + bootstrap timing + debug logging (v0.1.23)
- Shell prompt check + 2s delay before bootstrap (v0.1.24)
- Reliable bootstrap via process check + shell picker visibility (v0.1.25)
- Shorten Tailscale toggle label to prevent truncation (v0.1.26)
- Workspace created ONLY after onboarding completes (v0.1.27)
- Pass workspace name through FFI — `main` hardcoding removed (v0.1.28)
- Direct CLI execution via PTY — no bootstrap timing needed (v0.1.29–v0.1.30)
- Parse command string into binary + args for direct PTY execution (v0.1.31)
- Remove duplicate bootstrap (v0.1.32)
- Run CLI via login shell (`-l -c`) instead of direct exec (v0.1.33)
- Use login shell for CLI detection — app environment PATH is limited (v0.1.34)
- Trust user CLI selection — remove `cliAvailable` gate (v0.1.35)

---

## Release Notes — v0.1.46

### Highlights

**IPC Phase 1 Complete** — aterm now has a full IPC layer (`aterm-session` + `aterm-ipc` crates) enabling programmatic session management. The `bin/aterm` CLI exposes `list`, `create`, `kill`, `inject`, `dispatch`, `tasks`, and `lessons` commands. Sessions in aterm are first-class citizens reachable via Unix domain socket with token auth.

**GPU Renderer Overhaul** — The `wgpu`/`glyphon` rendering pipeline has been rewritten for reliability. Nine built-in color themes ship out of the box (Dark, Light, Solarized, Nord, Dracula, Monokai, One Dark, Gruvbox, Tokyo Night).

**Settings UI** — Cmd+, opens a native settings panel: font, line spacing, cursor style, theme, and language (English/Korean).

**Session Persistence in Rust** — Session state moved from Swift to Rust FFI with atomic writes and automatic migration. Platform UIs call into Rust for all persistence.

**Devkit Integration** — AGENTS.md and GEMINI.md generation delegated to `aigentry-devkit`, removing self-generation code from AppDelegate.

**Stability** — Fixed empty session names (#104), inject routing, session restore guard, CLI spawn wrapper, and npm packaging.

### Stats

- **38 files changed**, 3,847 insertions, 376 deletions
- **3 new crates**: `aterm-session`, `aterm-ipc`, `aterm-core` expanded
- **18 commits** since v0.1.39
