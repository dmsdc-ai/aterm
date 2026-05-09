# Aterm Phase 1 Chunk 2 Cross-LLM Review (2026-05-09)

Reviewer: Claude (cross-LLM verify of Codex impl).
Plan: `docs/superpowers/plans/2026-05-06-aterm-phase1-cleanup.md` (r3, `fb4a1cf`).
Branch: `phase1/cleanup-2026-05-06` @ `426115a54f05e9a0c512887acf3f7080c0086f0c`.

## §1 Verdict

**ACCEPT**

All 8 verification gates pass. Codex's REPORT is consistent with the on-disk evidence on every load-bearing claim (I9 lockfile pairing, lockfile prune count, workspace-only manifest, canonical 70/72 baseline, surgical scope). One out-of-scope informational finding noted in §4; it does not block Chunk 3 entry.

## §2 V1–V8 Verification table

| Gate | Check | Result | Evidence |
|------|-------|--------|----------|
| **V1** | Commit + branch state, I9 lockfile pair | ✅ | `git log -1 426115a --stat` → `Cargo.lock` + `Cargo.toml` only, paired in single commit. HEAD = `426115a` on `phase1/cleanup-2026-05-06`. I9 ✅. |
| **V2** | Consumer scans (R2-2 + R2-6, I10 deferral) | ✅ | `rg aterm-v3` matches: docs/plan + docs/reports (self-doc, allowed), `bin/run-debug.sh` + `scripts/package-aterm-v3-app.sh` (I10 deferred stale, allowlist), `archived/src-v3-future/macos/Info.plist` (Chunk 5 deletion target). No unexpected consumers. |
| **V3** | Root `Cargo.toml` workspace-only + lockfile prune | ✅ | `Cargo.toml` is **2 lines**: `[workspace]` + `members = ["aterm-core", "aterm-session", "aterm-ipc"]`. No `[package]`, no `[[bin]] aterm-v3`, no `.` in members, no wgpu/glyphon/pollster/winit deps. `Cargo.lock` diff: 305 `name = ` deletions − 76 `name = ` additions = **net −229** package entries, matching Codex report exactly. Confirmed removals: `aterm-v3`, `glyphon`, `pollster`, `wgpu`, `winit`. |
| **V4** | Canonical workspace baseline (R2-1 / R3-C3 reference) | ✅ | `/tmp/aterm-postchunk2-workspace-test.log` (42 605 B, 18:49). Suite tally: 52 + 18 + 0 + 0 + 0 + 0 + 0 = **70 passed / 2 failed / 72 total**. Failed tests: `ffi_tests::all_ffi_entry_points_wrapped_in_catch_unwind` + `inject::tests::inject_queue_rejects_when_full` — **identical pair** to Chunk 1 review (commit `98368c2`), both documented as FFI panic guard #363 in architect deep-analysis `8aeb21e`. Continuity ✅. |
| **V5** | `src-v3/` absent | ✅ | `ls src-v3` → "No such file or directory". No commit needed (per plan). |
| **V6** | `make app` smoke | ✅ | `build/aterm.app/Contents/MacOS/aterm` is a Mach-O 64-bit arm64 binary, 2 647 152 B, mtime 2026-05-09 18:51 (fresh, post-commit). Lesson F3 satisfied (binary is freshly produced, not stale). |
| **V7** | Working tree clean | ✅ | `git status --porcelain` → empty. No untracked leakage. |
| **V8** | Rule 29 surgical (allowlist) | ✅ | `426115a` touches exactly `Cargo.toml` + `Cargo.lock` — both explicitly in the Chunk 2 File Structure allowlist (I9). No drive-by edits. |

## §3 Canonical baseline log location

`/tmp/aterm-postchunk2-workspace-test.log` — **70/72** pass under `cargo test --workspace --no-fail-fast`.

This is the canonical reference for Chunk 3 (R2-1 regression check) and Chunk 6 (R3-C3 final-baseline diff). The 2 known failures (`all_ffi_entry_points_wrapped_in_catch_unwind`, `inject_queue_rejects_when_full`) are pre-existing #363 FFI-panic-guard failures and must be ignored when comparing Chunk 3 / Chunk 6 baselines.

## §4 New issues

### Informational only (out-of-scope for Chunk 2; do NOT block)

**S1.** `.github/workflows/test-install.yml:27` still runs `git clone … ../winit`, but the root `Cargo.toml` no longer has any winit dep (and no member crate uses winit either, post-removal). The CI clone is now a no-op build artifact left over from the v3 era — same fingerprint as the I10 deferred shell scripts. **Out of Chunk 2 allowlist** (CI workflow file is not in ADR §6.1), so editing it here would have been scope creep. Recommend opening a follow-up backlog item alongside `#TBD-aterm-v3-shell-script-cleanup` to drop the obsolete winit clone step + the workflow's path-dep environment. Ticket sketch: `#TBD-aterm-ci-winit-clone-cleanup`.

No other issues found.

## §5 Verdict + Next Action

- **Verdict:** ACCEPT
- **Proceed to Chunk 3:** **YES**
- **Conditions:** 0 (S1 is informational follow-up, not a Chunk 2 blocker).
- **Hand-off invariants for Chunk 3:**
  - Canonical baseline = `/tmp/aterm-postchunk2-workspace-test.log` (70/72).
  - I9 lockfile pairing now in force; any Chunk 3 manifest edit must `git add Cargo.lock` in the same commit.
  - I10 deferral active for `bin/run-debug.sh` + `scripts/package-aterm-v3-app.sh`.
