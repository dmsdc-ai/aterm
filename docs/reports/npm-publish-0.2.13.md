# Publish Log: @dmsdc-ai/aterm@0.2.13

**Date:** 2026-04-12
**Outcome:** SUCCESS

## Build #100
- EXIT=0, 0 errors, 0 new Swift warnings (18 SettingsView onChange deprecation unchanged from #99)
- Smoke: FONT-WARN fires, zero errors, PID 88025 RSS 140.9MB

## Publish
Both `@dmsdc-ai/aterm@0.2.13` and `@dmsdc-ai/aterm-darwin-arm64@0.2.13` live on registry.

## Install verification
Clean-dir: version 0.2.13, binary extracted OK.

## Changes
- Workspace dedup guard (+12 AppDelegate)
- telepty-bus 2s connect delay (+5 TeleptyBusClient)
- shell-ready 30s + codex patterns (pty.rs + inject.rs)
- codex default --last removed (SettingsView)
