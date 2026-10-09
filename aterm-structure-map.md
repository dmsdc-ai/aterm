# Aterm Structure Map

## Purpose & Status

Repository structure map after Phase 1 cleanup for ADR `~/projects/aigentry-orchestrator/docs/adr/2026-05-06-aterm-session-control-opt-3-prime.md`. Snapshot date: 2026-05-09. This is the source of truth for the next Phase 2 work session.

## Cargo Workspace Layout

- `aterm-core`: primary Rust library and C ABI surface for the macOS shell. Built as the native library consumed by Swift.
- `aterm-session`: shared session action and host/type contracts.
- `aterm-ipc`: local IPC server, authentication, and request routing support.
- Workspace root: `Cargo.toml` is workspace-only and declares `members = ["aterm-core", "aterm-session", "aterm-ipc"]`. There is no root crate.

## Build Pipeline

The Makefile pipeline is `make rust && make swift && make metal && make app`.

- `make rust` builds the release Rust native library from `aterm-core`.
- `make swift` compiles the AppKit shell from `macos/Sources/`, imports `macos/aterm-bridge.h`, links the native library, and links AppKit, Foundation, Metal, QuartzCore, and CoreVideo.
- `make metal` compiles `macos/Sources/Shaders.metal` into `build/default.metallib`.
- `make app` creates `build/aterm.app`, copies the Swift executable, native library, shader library, and bundled CLI helper, adjusts the native library install name, writes bundle metadata, and signs the app bundle.

Rendering is Metal in the Swift shell. Terminal state and PTY behavior live in the Rust workspace.

## Distribution

- `npm/aterm`: JavaScript launcher package and install helper.
- `npm/aterm-darwin-arm64`: macOS arm64 native package — the only platform package `postinstall.js` can resolve.

There is no root package manifest. Package manifests now live under `npm/`; the launcher manifest is `npm/aterm/package.json`.

## File Roles

Rust (`aterm-core/src/`):
- `lib.rs`: the C ABI consumed by Swift (`#[no_mangle]` exports, declared by hand in `macos/aterm-bridge.h`; `ffi_tests::bridge_header_matches_exports` keeps the two in sync), `ffi_catch!` panic guards, `aterm_drain_events`.
- `app.rs`: `AtermApp` singleton. Owns the IPC server, routes `SessionAction`s (create/close/inject/status/wait/mark-complete) and broadcasts events with a sequence number.
- `pty.rs`: `PtyManager`. Spawns and restarts workspace shells, runs the PTY reader loop (NFC normalisation, OSC 133, trust-prompt and codex-resume monitors), resolves CLI binaries.
- `terminal.rs`: `alacritty_terminal` state, prompt marks and the render-data feed.
- `inject.rs`: inject queue, prompt/OSC 133 readiness detection and delivery.
- `session.rs`: workspace session registry and persistence under the data root (`ATERM_DATA_ROOT`, default `~/.aigentry`), with the legacy-format migrations.
- `telepty_bridge.rs`: optional, fire-and-forget registration of workspaces with the telepty daemon; hermetic guards keep tests off the production port.
- `tailscale.rs`: embedded Tailscale node (`tsnet`).
- `mailbox/`: file-backed mirror of inject messages.

Rust (other crates):
- `aterm-session/src/`: `action.rs` (IPC wire contract), `host.rs` (`PlatformHost`), `types.rs`.
- `aterm-ipc/src/`: `server.rs` (Unix-socket NDJSON server and subscribers), `auth.rs` (peer-UID check).

Rust tests (`aterm-core/tests/`): `cli_dispatch.rs` drives `bin/aterm` against a fake socket; `hermetic_guard.rs` and `telepty_no_restart.rs` hold the telepty safety contract.

Swift (`macos/Sources/`):
- `AppDelegate.swift`: window, sidebar + terminal container, workspace lifecycle, telepty daemon start and deregister on quit.
- `TerminalView.swift`: `NSView` + `NSTextInputClient` (IME), input and render loop.
- `MetalRenderer.swift`, `Shaders.metal`, `GlyphAtlas.swift`: Metal rendering and Core Text glyph atlas.
- `SessionSidebarView.swift`, `SettingsView.swift`, `OnboardingView.swift`, `AtermTheme.swift`: UI, settings and theme.
- `OrchestratorInputBar.swift`, `OrchestratorCommands.swift`, `OrchestratorHistory.swift`: orchestrator input bar.
- `TeleptyBusClient.swift`, `TeleptyAuth.swift`: telepty bus WebSocket client and its auth token.

CLI and packaging:
- `bin/aterm`: bash + python3 CLI over `$ATERM_IPC_SOCKET`, telepty fallback outside aterm.
- `npm/aterm/bin/aterm.js`: launcher. Runs the bundled CLI for subcommands, otherwise opens the app.
- `npm/aterm/scripts/postinstall.js`, `npm/aterm/lib/aigentry.js`: install-time layout and config.
- `npm/prepare-platform-package.mjs`: runs `make app` and copies the bundle into the platform package.
- `scripts/generate-app-icons.py`: app icon asset pipeline.

Verification: `state/tests.md` (inventory generated from `cargo test -- --list`) and `state/verification-families/` (`runner.sh` packs and profiles).
