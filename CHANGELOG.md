# Changelog

All notable changes to **aterm** are documented in this file.

---

## Unreleased

### Changed — aterm authenticates to the telepty daemon (task #825)
- All telepty daemon calls now send the daemon's auth token as an `x-telepty-token`
  header: the Rust bridge's session list, register and deregister calls, the Swift
  deregister and daemon-probe calls, and the `/api/bus` WebSocket upgrade.
- The token is read from `authToken` in `~/.telepty/config.json`, resolved per
  request so a config written after aterm launches is still picked up.
  `TELEPTY_AUTH_TOKEN` overrides it when set. A missing or malformed config
  degrades to no header rather than failing the app.
- `/api/health` is unchanged and remains unauthenticated.

### Changed — License clarified to MIT (task #456)
- npm packages now explicitly declare `"license": "MIT"` instead of `"UNLICENSED"` metadata.
- Cargo workspace and all crates now declare MIT license metadata.
- Added the root `LICENSE` file.
- Source: orchestrator task #456, follow-up to herdr.dev comparison finding #4.

---

## 0.2.13 — 2026-04-12

### Fixes
- Workspace dedup: prevent duplicate workspace creation when external project scan detects already-existing internal workspace (name+cwd guard)
- telepty-bus race: 2s connect delay after daemon start prevents premature WebSocket failure
- shell-ready timeout 10s→30s: accommodates slow codex resume initialization
- codex prompt patterns: 'Summarize the last session' and 'What would you like to do?' added to shell-ready detection
- codex default args: removed --last from resume command — first launch starts fresh codex session instead of failing on missing history

---

## 0.2.12 — 2026-04-12

### Fixes
- Force Dark appearance for aterm UI chrome (sidebar, Settings, window frame) regardless of macOS system appearance setting
- devkit workspace-init PATH expansion: npm global bin paths prepended for Finder-launched .app (fixes exit 127 for most workspaces, 9→5 remaining)
- SettingsView onChange deprecation closures migrated to one-arg signature (18/18 closures updated)

---

## 0.2.11 — 2026-04-12

### Fixes
- **Global cursor CJK alignment**: cell height now accounts for CJK fallback font metrics (cellH 27→29), fixing cursor top-alignment for Korean/Chinese/Japanese text
- **Auto-restart retry strip**: resume flags (--continue, --last, resume) now stripped on workspace restart, preventing infinite failure loops when no prior session exists
- **P0 default workspace**: orchestrator workspace auto-selected on launch (previously selected last entry)
- **OrchestratorInputBar v3.1**: transparent edge-to-edge design with 1px hairline top separator, no border/corner radius/margins
- **OrchestratorInputBar CJK centering**: dynamic line height measurement for Korean/CJK text vertical centering
- **Popover focus defense**: 3-layer fix preventing NSPopover from stealing first responder (filter-as-you-type, Tab, Enter all work in command dropdown)
- **Settings UX**: theme tiles and tab buttons now respond to full-area clicks (contentShape fix)
- **Settings macOS 14 API**: onChange closure compatibility for pre-macOS 14 deployment targets
- **Diagnostic logs removed**: all [CURSOR-DIAG] temporary logging removed for clean release

### Features
- **Active session scope**: aterm list returns union of internal + telepty sessions by default. New flags: --all, --internal-only, --telepty-only, --json. SOURCE column added.
- **Settings 4 gap**: Font Family picker with Show-all-fonts toggle, Shell picker (zsh/bash/fish/custom), Tailscale on-launch toggle, Orchestrator args visible for all CLIs
- **Variant badge**: lazy detection warns when selected font lacks Bold/Italic variants
- **Font plumbing hot-reload**: changing font family applies immediately without restart. 4-tier fallback chain (userFixedPitchFont → Menlo → SFMono → systemFont)

### Breaking
- aterm list output now includes SOURCE column (appended, existing column order preserved)
- ATERM_LIST_JSON=1 env var no longer honored (use --json flag)

---

## 0.2.10 — 2026-04-12

### Fixes
- **P0**: default workspace selection no longer biased to last entry — orchestrator (isSystem) is now auto-selected on launch. Previously users had to manually click the orchestrator row after launch. (AppDelegate.swift:703)
- **#240**: OrchestratorInputBar cursor vertical centering root fix + popover focus 3-layer defense + /command dropdown Enter/filter/Tab now work correctly
- **#246**: Default UI language is English. Korean translations preserved for future multi-language support
- Settings tab click hit area extended to full cell (previously only icon/label was clickable)
- Theme picker now applies live on click instead of waiting for Save
- Font plumbing hot-reload: changing font family in Settings applies immediately without restart
- Variant badge lazy detection shows warning when selected font lacks Bold/Italic variants
- SettingsView macOS 14+ API usage downgraded via 3-line closure-signature fix for compatibility

### Features
- `aterm list` now returns union of internal workspaces + telepty external sessions by default
- New flags: `--all` / `--internal-only` / `--telepty-only` / `--json`
- New `SOURCE` column identifies each entry origin

### Breaking
- `ATERM_LIST_JSON=1` env var no longer honored. Use `--json` flag instead.

### Known Issues
- `claude` CLI may not auto-spawn for orchestrator workspace on first launch (manual `claude` run works as a workaround)
- Unsigned binary: install via postinstall automatically removes Gatekeeper quarantine (`xattr -cr`)
- InputBar v3 modern redesign pending (Direction E, future 0.2.11 or 0.3.0)

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
