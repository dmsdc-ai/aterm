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
- `npm/aterm-darwin-arm64`: macOS arm64 native package.
- `npm/aterm-darwin-x64`: macOS x64 native package.
- `npm/aterm-linux-arm64`: Linux arm64 package.

There is no root package manifest. Package manifests now live under `npm/`; the launcher manifest is `npm/aterm/package.json`.

## Phase 2 Entry Points

ADR section 6.2 names the next file scope: `aterm-core/src/app.rs` for `SessionAction::AttachExternal`, `aterm-core/src/telepty_bridge.rs`, and the `bin/aterm.js` inject alias. Those files are cited here only as Phase 2 references and were not changed by this map.

## Known Follow-ups

- 28 `cfg(not(feature = "wgpu"))` gates remain in `aterm-core/src/lib.rs` per R2-5 deferral. An un-gate cleanup is pending.
- `bin/run-debug.sh` and `scripts/package-aterm-v3-app.sh` reference the removed aterm-v3 ghost per R2-2 Option B / I10 deferral. Cleanup task `#TBD-aterm-v3-shell-script-cleanup` is pending orchestrator backlog registration.
