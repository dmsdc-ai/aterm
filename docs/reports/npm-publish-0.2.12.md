# Publish Log: @dmsdc-ai/aterm@0.2.12

**Date:** 2026-04-12
**Outcome:** SUCCESS

## Build #99
- EXIT=0, 0 errors
- 18 SettingsView onChange deprecation warnings (sub1's "cleanup" used one-arg `onChange(of:perform:)` which IS the deprecated API on macOS 14+ deployment target; sub1's narrow typecheck under macOS 13 target didn't see these)
- Warning count: 3 (#98) → 18 (#99) — regression in warning count, not functionality
- 75 rust wgpu dead-code warnings (unchanged)

## Smoke test
- FONT-WARN fires, zero errors
- devkit workspace-init exit 127: 9 (#88) → 5 (#99) — PATH expansion partially effective

## Publish
| Package | Version | Shasum |
|---|---|---|
| `@dmsdc-ai/aterm` | 0.2.12 | verified live |
| `@dmsdc-ai/aterm-darwin-arm64` | 0.2.12 | verified live |

## Install verification
- Clean-dir: 0.2.12, binary extracted, quarantine cleared (only com.apple.provenance)

## Concerns
1. 18 onChange deprecation warnings — sub1 migrated TO the deprecated form (one-arg) instead of the modern form (zero-arg or two-arg). Under macOS 14+ these warn. Non-blocking but a true fix would use `.onChange(of:value) { ... }` (zero-arg closure).
2. devkit 127 reduced but not eliminated (5 remaining) — some workspaces still can't find devkit binary even with expanded PATH.
