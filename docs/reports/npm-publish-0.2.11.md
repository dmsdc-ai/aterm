# Publish Log: @dmsdc-ai/aterm@0.2.11 + @dmsdc-ai/aterm-darwin-arm64@0.2.11

**Date:** 2026-04-12
**Publisher:** aigentry-builder-claude (duckyoung_kim on npm)
**Outcome:** SUCCESS — both packages live, install verified, cellH=29 confirmed in build

## Summary

Final release candidate build (#98) with all 13 fix/feature items from the 0.2.11 sprint. Build clean (EXIT=0, 0 errors, 3 expected SettingsView deprecation warnings). CURSOR-DIAG logs confirmed removed (0 lines in smoke test). Bundle size 20MB. cellH=29 from narrowed CJK probe verified in prior rebuild #97.

## Included in 0.2.11 (delta from 0.2.10)

1. P0 default workspace fix (AppDelegate shouldSelectIndex)
2. Active session scope (bin/aterm union + flags)
3. v3.1 OrchestratorInputBar transparent edge-to-edge
4. Global cursor CJK fix (cellH 27→29 via CTFont CJK probe max + ceil)
5. OrchestratorInputBar CJK-aware lineHeight (Fix A)
6. Auto-restart retry strip (pty.rs)
7. Diagnostic logs removed (7 CURSOR-DIAG cleaned from 6 files)
8. Popover focus 3-layer defense
9. Settings UX (contentShape)
10. SettingsView macOS 14 API fix
11. Variant badge lazy detection
12. Font plumbing hot-reload (4-tier fallback chain)
13. Settings 4 gap (fontFamily/shell/tailscale/orchestrator.args)

## Build verification

```
EXIT=0, 0 errors, 3 SettingsView onChange deprecation warnings (expected, non-blocking)
Bundle size: 20 MB
Smoke test: launched cleanly, FONT-WARN fires, CURSOR-DIAG absent (removed), zero errors
```

## Publish

| Package | Version | Size | Shasum |
|---|---|---|---|
| `@dmsdc-ai/aterm-darwin-arm64` | 0.2.11 | 7.9 MB | `852dbe5d14defeabe8bd38519749156d3d2753b9` |
| `@dmsdc-ai/aterm` | 0.2.11 | 5.8 KB | `a476537a5eea63521b0a0ab2dc39f2d7f418c749` |

Published in order: platform first, then wrapper. Both confirmed live via `npm view`.

## Install verification

Clean-dir install:
- `npm install @dmsdc-ai/aterm@0.2.11` → resolved both packages + devkit + telepty deps
- Wrapper version in node_modules: `0.2.11` ✓
- Binary extracted: `dist/aterm.app/Contents/MacOS/aterm` (2.68 MB executable) ✓
- `xattr` check: only `com.apple.provenance` (quarantine removed by postinstall `xattr -cr`) ✓

## CHANGELOG

0.2.11 entry prepended to `~/projects/aigentry-aterm/CHANGELOG.md` with full Fixes/Features/Breaking sections.

## Artifacts

- Build log: `/tmp/rebuild-98-build.log`
- Smoke log: `/tmp/rebuild-98-smoke.log`
- Registry: https://registry.npmjs.org/@dmsdc-ai/aterm/-/aterm-0.2.11.tgz
- Registry: https://registry.npmjs.org/@dmsdc-ai/aterm-darwin-arm64/-/aterm-darwin-arm64-0.2.11.tgz
