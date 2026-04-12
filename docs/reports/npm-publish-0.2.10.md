# Publish Log: @dmsdc-ai/aterm@0.2.10 + @dmsdc-ai/aterm-darwin-arm64@0.2.10

**Date:** 2026-04-12
**Publisher:** aigentry-builder-claude (duckyoung_kim on npm)
**Outcome:** SUCCESS — both packages live on registry, clean-dir install + binary launch verified

## Version decision

- Previous published: `@dmsdc-ai/aterm@0.2.9` (prior builder inject had incorrectly proposed `@dmsdc-ai/aigentry-aterm@0.0.1`; orchestrator corrected to patch-bump of existing package).
- This release: **`0.2.10`** (patch bump, user decision "응 해결하고 패치만 올려줘").
- Both wrapper and platform package published in lockstep.

## Scope

Merging a substantial batch of sub1 + aterm-claude fixes landing in one patch:

1. **P0 default workspace selection fix** (AppDelegate.swift:703; aterm-claude REPORT 026480ad)
   - `let shouldSelectIndex = entries.firstIndex(where: { $0.isSystem })`
   - `shouldSelect: i == shouldSelectIndex` (replaces hardcoded `true`)
2. **#240 OrchestratorInputBar redesign** (cursor vertical centering + popover focus 3-layer + /command dropdown; SPEC ref ea1d288d)
3. **#246 English default locale** (AtermLocalization 3-line patch; sub1 REPORT 9273579a)
4. **Active session scope fix** (bin/aterm; aterm-claude REPORT ece4b2de)
   - Union of internal workspaces + telepty external sessions by default
   - New flags: `--all` / `--internal-only` / `--telepty-only` / `--json`
   - New `SOURCE` column
   - **Breaking:** `ATERM_LIST_JSON=1` env var removed, use `--json` flag
5. **Settings UX fixes** (sub1 REPORT 82f289e2)
   - `contentShape(Rectangle())` on SchemeButton and SettingsTab button label for full hit area
6. **Variant badge lazy detection** (sub1 REPORT 2e85ebd7)
7. **Font plumbing hot-reload** (aterm-claude REPORT bda483ae; 4-tier fallback chain)
8. **SettingsView macOS 14+ API fix** (sub1 incoming — 3-line closure-signature downgrade, full-project swiftc PASS)
9. **popover focus 3-layer defense** (DropdownTableView.acceptsFirstResponder=false + popoverDidShow observer + anchorWindow.makeKey restore + NSPanel becomesKeyOnlyIfNeeded cast, prior task from #87)

Plus all previously-landed #86/#87/#88 work.

## Pre-publish verification

### Rebuild #89 final

```
cd ~/projects/aigentry-aterm && \
  touch macos/Sources/SettingsView.swift macos/Sources/AppDelegate.swift && \
  make app > /tmp/rebuild-89-final-build.log 2>&1
EXIT=0
```

- Swift: **0 errors, 0 warnings** across 14 source files (including SettingsView macOS 14+ API fix from sub1, P0 fix from aterm-claude)
- Rust: 75 legacy wgpu-cfg dead-code warnings (unchanged, known)
- Final: `[build] App bundle created: /Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app`
- Bundle size: **20 MB** (2.7 MB Swift binary, 17 MB libaterm_core.dylib, 484 KB Resources)

### Sandbox smoke launch of #89

PID 12500, RSS 125.8 MB, CPU 0.0% (idle), `[FONT-WARN] fallback chain` fires (font plumbing preserved), zero errors.

## Packaging

Existing production-grade infrastructure at `~/projects/aigentry-aterm/npm/` was reused:
- Split-package pattern (wrapper + optional platform dep, esbuild/biome style)
- `prepare-platform-package.mjs` (runs `make app` + stages bundle via `prepack` hook)
- Sophisticated postinstall (system/user/project layout + AI CLI detection + devkit bootstrap + bundle extraction)
- Launcher handles `DYLD_LIBRARY_PATH` + `AIGENTRY_CONFIG_*` env + single-instance activation + `ATERM_DATA_ROOT` sandbox passthrough

### Version bumps

```diff
# npm/aterm/package.json
- "version": "0.2.9"
+ "version": "0.2.10"
- "@dmsdc-ai/aterm-darwin-arm64": "0.2.9"
+ "@dmsdc-ai/aterm-darwin-arm64": "0.2.10"

# npm/aterm-darwin-arm64/package.json
- "version": "0.2.9"
+ "version": "0.2.10"
```

### Postinstall `xattr -cr` addition

Added best-effort Gatekeeper quarantine removal after bundle copy in `npm/aterm/scripts/postinstall.js`, to ensure the unsigned binary launches cleanly on company MacBooks where the user cannot self-sign:

```javascript
try {
  const { execFileSync } = require('node:child_process');
  execFileSync('xattr', ['-cr', targetApp], { stdio: 'ignore' });
} catch {
  // xattr absence or permission denial — leave quarantine in place.
  // User can run `xattr -cr <path>` manually if Gatekeeper blocks launch.
}
```

**Verified effective** during install smoke test (see below) — `com.apple.quarantine` attribute is absent from the installed bundle; only `com.apple.provenance` (macOS-level file tracking, NOT the Gatekeeper flag) remains.

### Pack dry-run sizes

| Package | Packed | Unpacked | Files |
|---|---|---|---|
| `@dmsdc-ai/aterm-darwin-arm64@0.2.10` | **7.9 MB** | 20.6 MB | 8 |
| `@dmsdc-ai/aterm@0.2.10` | **5.8 KB** | 21.1 KB | 4 |

Both well under the 50 MB npm registry limit.

## Publish

Publish order matters (wrapper declares optional dependency on the platform package):

### 1. `@dmsdc-ai/aterm-darwin-arm64@0.2.10`

```
cd ~/projects/aigentry-aterm/npm/aterm-darwin-arm64 && npm publish --access public
...
+ @dmsdc-ai/aterm-darwin-arm64@0.2.10
```

Tarball contents (8 files, 7.9 MB):
- `dist/aterm.app/Contents/_CodeSignature/CodeResources` (3.1 kB, ad-hoc)
- `dist/aterm.app/Contents/Frameworks/libaterm_core.dylib` (17.4 MB)
- `dist/aterm.app/Contents/Info.plist` (746 B)
- `dist/aterm.app/Contents/MacOS/aterm` (2.7 MB)
- `dist/aterm.app/Contents/Resources/AppIcon.icns` (394.8 kB)
- `dist/aterm.app/Contents/Resources/bin/aterm` (49.4 kB)
- `dist/aterm.app/Contents/Resources/default.metallib` (44.4 kB)
- `package.json` (439 B)

Shasum: `8198d5ede9a6d127630f44ca63f4be64c3be502f`

### 2. `@dmsdc-ai/aterm@0.2.10`

```
cd ~/projects/aigentry-aterm/npm/aterm && npm publish --access public
...
+ @dmsdc-ai/aterm@0.2.10
```

Tarball contents (4 files, 5.8 kB):
- `bin/aterm.js` (2.9 kB)
- `lib/aigentry.js` (11.6 kB)
- `package.json` (764 B)
- `scripts/postinstall.js` (5.8 kB — includes new `xattr -cr` hook)

Shasum: `1a41b75091c9972128224d5d1dce9685cb4c11b6`

## Post-publish verification

### Registry check

```
$ npm view @dmsdc-ai/aterm@0.2.10 version dist.tarball dist.shasum
version = '0.2.10'
dist.tarball = 'https://registry.npmjs.org/@dmsdc-ai/aterm/-/aterm-0.2.10.tgz'
dist.shasum = '1a41b75091c9972128224d5d1dce9685cb4c11b6'

$ npm view @dmsdc-ai/aterm-darwin-arm64@0.2.10 version dist.tarball dist.shasum
version = '0.2.10'
dist.tarball = 'https://registry.npmjs.org/@dmsdc-ai/aterm-darwin-arm64/-/aterm-darwin-arm64-0.2.10.tgz'
dist.shasum = '8198d5ede9a6d127630f44ca63f4be64c3be502f'
```

Both shasums match locally computed values → registry has exactly the artifacts we built.

### Clean-dir install smoke

```
$ mkdir -p $(mktemp -d -t aterm-test-0210-XXXXXX) && cd $_ && npm init -y && \
  npm install @dmsdc-ai/aterm@0.2.10

Installed: @dmsdc-ai/aterm@0.2.10 + @dmsdc-ai/aterm-darwin-arm64@0.2.10
          + @dmsdc-ai/aigentry-devkit (dependency)
          + @dmsdc-ai/aigentry-telepty (dependency)
```

- Wrapper version in node_modules: `0.2.10` ✓
- Platform bundle extracted to `node_modules/@dmsdc-ai/aterm/dist/aterm.app/Contents/MacOS/aterm` (2.67 MB executable) ✓
- `xattr` after install: only `com.apple.provenance` (quarantine absent, Gatekeeper will not block first launch) ✓

### Installed binary launch smoke

```
ATERM_DATA_ROOT=/tmp/aterm-smoke-data ATERM_TELEPTY_PORT=13849 \
  $INSTALL_DIR/node_modules/@dmsdc-ai/aterm/dist/aterm.app/Contents/MacOS/aterm
```

Log excerpt:
```
[aterm] sandbox mode (ATERM_DATA_ROOT set), skipping single-instance check
[telepty-bus] connecting to ws://127.0.0.1:13849/api/bus
[telepty-bus] WebSocket error: Could not connect to the server.
[aterm] starting telepty daemon from /opt/homebrew/bin/telepty
[aterm] telepty daemon started (pid 36235)
[aterm] first install — no sessions to restore
[aterm-ipc] listening on /tmp/aterm-36189.sock
[aterm-ipc] host callbacks registered
```

- Binary launched ✓
- PID 36189, RSS 108.5 MB (fresh-start, no workspaces yet) ✓
- Auto-started its own telepty daemon when port 13849 had none ✓
- IPC server listening ✓
- Clean first-install path ("no sessions to restore") ✓

Smoke process killed after confirmation. The pre-existing Rebuild #89 sandbox (PID 12500) remains running undisturbed.

## CHANGELOG

Updated `~/projects/aigentry-aterm/CHANGELOG.md` with the 0.2.10 entry matching the orchestrator-specified format (Fixes / Features / Breaking / Known Issues sections). The CHANGELOG.md file is at the repo root and is NOT included in the npm tarball `files` array for either package — users will find it on GitHub. If bundling it inside the package becomes desirable later, add `"CHANGELOG.md"` to the wrapper package.json `files` array and copy from the repo root during the publish step.

## Concerns / follow-ups

1. **Unsigned binary + Gatekeeper** — `xattr -cr` in postinstall is a workaround, not a proper fix. The company MacBook scenario is covered but proper Developer ID signing + notarization is still the right long-term solution. MVP accepted tradeoff.

2. **`com.apple.provenance` attribute** — still present on the installed bundle after `xattr -cr`. This is NOT the Gatekeeper quarantine attribute; it's macOS file-origin tracking. Not a blocker, but if any edge case surfaces it can be cleared with `xattr -d com.apple.provenance <path>` (requires SIP-disabled root on some macOS versions).

3. **Breaking change `ATERM_LIST_JSON=1` removal** — documented in CHANGELOG Breaking section. No known downstream scripts in this repo rely on it (grepped), but external users with their own automation would need to switch to `--json`. Patch version (0.2.10) is arguably the wrong place for a breaking change — SemVer would push this to 0.3.0. The user explicitly requested patch-only. Flagging.

4. **P0 fix is source-verified but requires GUI validation** — tester/user must click orchestrator row and confirm the input bar is visible. Source lines 687-713 match the spec a0619e111924 exactly, but GUI hit-test can always surprise.

5. **x64 platform package is stub** — `~/projects/aigentry-aterm/npm/aterm-darwin-x64/` exists but has no native bundle. Apple Silicon is assumed for this MVP. Intel Mac users installing `@dmsdc-ai/aterm` will fail cleanly with "no native package for darwin x64; install skipped" message from postinstall.

6. **`com.apple.provenance` observation** — install ran without errors, but if any corporate MacBook has stricter MDM policies stripping npm's postinstall privileges, `xattr -cr` may not run. Fallback manual command documented in CHANGELOG.

## Artifacts

- Build log: `/tmp/rebuild-89-final-build.log`
- Launch log (sandbox #89): `/tmp/aterm-debug-live.log`
- Smoke-test launch log: `/tmp/aterm-smoke.log`
- Repo CHANGELOG: `~/projects/aigentry-aterm/CHANGELOG.md`
- App bundle: `~/projects/aigentry-aterm/build/aterm.app` (20 MB)
- Running sandbox #89: PID 12500
- Registry tarballs:
  - https://registry.npmjs.org/@dmsdc-ai/aterm/-/aterm-0.2.10.tgz
  - https://registry.npmjs.org/@dmsdc-ai/aterm-darwin-arm64/-/aterm-darwin-arm64-0.2.10.tgz
