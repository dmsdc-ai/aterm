---
revision: r3
revision_history:
  - r1: 2026-05-06 — initial plan, plan-document-reviewer (claude) APPROVED iter 2 (commit `87b0c62`)
  - r2: 2026-05-08 — patch in response to codex cross-LLM review (`~/projects/aigentry-architect/docs/reports/2026-05-06-aterm-phase1-plan-codex-review.md`), verdict REQUEST_CHANGES, 7 conditions (2 BLOCKER + 5 MAJOR + 1 MEDIUM); architect r2 fixes applied in-place. NO new architectural decisions. ADR §6.1 scope unchanged.
  - r3: 2026-05-09 — minor patch for codex r2 focused review (`~/projects/aigentry-architect/docs/reports/2026-05-06-aterm-phase1-plan-r2-focused-review.md`), verdict ACCEPT_WITH_CONDITIONS, 3 minor text-only fixes (no architectural change). C1: WIP relocation moved to new Task 1.2 Step 1 (BEFORE the clean-tree gate); existing Task 1.2 steps renumbered 1→2, 2→3, 3→4. C2 (Option B): Task 3.5 Step 4 final wgpu verification keeps two separate greps (positive + negative) with explicit expected values `positive=0, negative=28`; the wrong "all matches: positive + negative substring" comment removed. C3: Task 3.5 Step 6 + Task 6.1 Step 1 baseline references updated from "Chunk 1 baseline" (active-crates only — invalid as a `--workspace` comparison) to "Chunk 2 Task 2.2 Step 4 canonical workspace baseline". ADR §6.1 scope unchanged.
---

# Aterm Phase 1 Cleanup Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove ghost crate, dead wgpu renderer (~3621 LOC), Tauri/Svelte residue, and ~5GB `archived/` so future Phase 2 feature work in `aigentry-aterm` lands on an unambiguous codebase (Rule 29 surgical pre-req).

**Architecture:** This plan is a **deletion-only** cleanup. Each chunk produces a green `cargo test --workspace` + `npm test` + `make app` baseline before moving on, with one git commit per task so any individual deletion can be reverted with `git revert <sha>`. No behavior change is introduced — every removed surface is reachable-from-zero (gated on the undefined `wgpu` feature, the missing `src-v3/main.rs`, archived dirs, or the v2 Tauri/Svelte stack mandated dead by ADR §3.4.1).

**Tech Stack:** Rust 1.x (cargo workspace `.`, `aterm-core`, `aterm-session`, `aterm-ipc`), Swift (macos/), Node (npm/aterm). Build: `make app` (Rust cdylib → swiftc → .app bundle). Test: `cargo test --workspace` + per-package `npm test`. **No new code is written in this plan.**

---

## Source of Truth

- **Binding ADR:** `~/projects/aigentry-orchestrator/docs/adr/2026-05-06-aterm-session-control-opt-3-prime.md` — commit `137aa96`, status=`accepted` (verified at plan-write time).
- **Phase scope:** ADR §6.1 only. Phase 2 (AttachExternal activation, telepty sub-PR) is **out of scope** for this plan and gated on Phase 1 acceptance per ADR §6.2.
- **Backlog mapping:** Closes `#355` (Ghost crate + dead wgpu) and `#356` (Tauri/Svelte residue).
- **Cross-LLM synthesis:** `~/projects/aigentry-orchestrator/docs/reports/2026-05-05-aterm-cross-llm-synthesis.md`
- **A-architect deep analysis:** `~/projects/aigentry-aterm/docs/reports/2026-05-06-aterm-session-control-architecture.md` (commit `8aeb21e`)
- **Memory cross-refs:** `feedback_aterm_v3_only.md`, `feedback_aterm_philosophy.md`
- **Constitution:** Articles 1 (경량) + 17 (무의존)

---

## Invariants (HARD — implementer must enforce)

| ID | Invariant | Enforcement signal |
|----|-----------|---|
| **I1** | NO behavior change — every deletion is unreachable code today. | Any post-deletion `cargo build` or `cargo test` regression aborts the chunk → revert + open question to architect. |
| **I2** | Surgical (Rule 29) — only files listed in ADR §6.1. NO drive-by formatting / refactor / "while I'm here" edits. | Reviewer rejects any diff line outside the file allowlist (see File Structure below). |
| **I3** | Every delete is preceded by a "verify no consumer" step (`rg <basename>`). Lesson F1. | Skipping → reviewer reject. |
| **I4** | Cargo / npm / Make baselines are GREEN before delete and GREEN after delete. Lesson F2. | Any RED → revert chunk to last green. |
| **I5** | One git commit per task (frequent commits). Each chunk ends on a green commit so individual reverts are trivial. | Multi-task commits → reviewer reject. |
| **I6** | NO scope creep beyond Phase 1. Phase 2 file list (ADR §6.2) is **NOT** touched. | Touching `app.rs:851-860`, `telepty_bridge.rs`, `bin/aterm.js`, `inject` alias etc. → reviewer reject. |
| **I7** | `archived/` git history MUST persist (already in past commits). Local working-tree removal is fine; **no `git filter-repo` or history rewrite**. Lesson F3. | History-rewrite command in plan → reviewer reject. |
| **I8** | NO new dependencies, NO version bumps. The plan only **removes** entries from `Cargo.toml` and root `package.json`. | Any `+` dep line in diff → reviewer reject. |
| **I5-exception (documented)** | Two documented multi-file commits are allowed: (a) Chunk 3 Task 3.5 Step 8 produces a single multi-file commit (3 file deletes + 1 lib.rs edit) because partial commits would leave the workspace red, violating I4. (b) **Any task whose manifest edit triggers a `Cargo.lock` diff** (notably Chunk 2 Task 2.2 Step 7) is paired (manifest + Cargo.lock in one commit) per I9. No other multi-file commits are permitted. | Any multi-file commit outside (a) and (b) → reviewer reject. |
| **I9 (Cargo.lock — R2-3 Option A)** | `Cargo.lock` is in the modification allowlist. Whenever a manifest edit triggers a lockfile diff (notably Chunk 2 Task 2.2 root manifest rewrite, Chunk 3 Task 3.5 if any dep is touched), `git add Cargo.lock` is added to the **same** commit as the manifest change. Lockfile diffs may NOT land in a separate commit, and may NOT be left uncommitted between chunks. After Chunk 2, the lockfile is valid and `cargo check --locked` would pass — BUT the plan does not require `--locked` because lockfile drift is expected during execution. | Standalone `Cargo.lock` commit OR uncommitted lockfile diff at chunk boundary → reviewer reject. |
| **I10 (Stale shell consumers — R2-2 Option B deferral)** | `bin/run-debug.sh` and `scripts/package-aterm-v3-app.sh` reference the removed `--bin aterm-v3` / `src-v3/` / `src-tauri/` surface and are **already broken** (build the absent ghost, copy non-existent assets). They are **out of Phase 1 scope** (not in ADR §6.1) and are explicitly deferred to a follow-up cleanup task. The plan must NOT delete or modify them; consumer scans must explicitly allow them. | Editing `bin/run-debug.sh` or `scripts/package-aterm-v3-app.sh` in this plan → reviewer reject (treat as scope creep). Follow-up backlog ticket: `#TBD-aterm-v3-shell-script-cleanup` (orchestrator to register; cite in Phase 1 final report). |

---

## File Structure

**Files to DELETE (terminal — gone after this plan):**

| Path | LOC / size | Reason |
|---|---:|---|
| `aterm-core/src/renderer.rs` | 2613 | Dead wgpu renderer; gated on undefined `feature = "wgpu"` (aterm-core/Cargo.toml has no `[features]` block and no `wgpu` dep). ADR §6.1 row 2. |
| `aterm-core/src/renderer_atlas.rs` | 385 | Same as above. ADR §6.1 row 3. |
| `aterm-core/src/renderer_glyph.rs` | 623 | Same as above. ADR §6.1 row 4. |
| `archived/src-v3-future/` | (part of 5.1GB) | Pre-v3 reference dump. ADR §6.1 row 7. Git history retained. |
| `archived/src-tauri-v1/` | (part of 5.1GB) | Pre-v3 Tauri reference. ADR §6.1 row 8. Git history retained. |
| `archived/.DS_Store` + any other `archived/*` subdirs | trace | Sweep — any subdir under `archived/` is by definition obsolete. |
| `package.json` (root) | 715 B | Tauri/Svelte residue per ADR §3.4.1 v3-only mandate. ADR §6.1 row 9. (npm-published `npm/aterm/package.json` is **untouched**.) |
| `package-lock.json` (root) | ~75 KB | Paired with root `package.json`. ADR §6.1 row 10. |
| `index.html` (root) | 2 KB | Vite/Tauri entrypoint. ADR §6.1 row 9. |
| `vite.config.js` (root) | (small) | Vite config. ADR §6.1 row 9. |
| `src/` (root) | full dir (App.svelte 26 KB + components/ + lib/ + main.js + design/ + app.css) | Svelte UI stack. ADR §6.1 row 9. |
| `scripts/patch-xterm-wk-ime.mjs` | ~10 KB | Patches `node_modules/@xterm/xterm@6.1.0-beta.195` which only exists under removed v2 stack. ADR §6.1 row 11 (codex C5). |
| `node_modules/` (root) | (build artifact) | Will be orphaned after `package.json` removal. Already gitignored. Delete from working tree only. |

**Files to MODIFY (kept, edited):**

| Path | Change | Reason |
|---|---|---|
| `Cargo.toml` (root) | Remove `[[bin]] aterm-v3` (lines 10-12). Remove unused root deps `wgpu`, `glyphon`, `winit`, `pollster` (lines 17, 18, 23, 24). Drop the entire root `[package]` block + `[dependencies]` + `[target.'cfg(target_os = "macos")'.dependencies]` since the workspace root carries **no source** (`src-v3/` is empty, no library either). The file collapses to a `[workspace]`-only manifest with `members = ["aterm-core", "aterm-session", "aterm-ipc"]` (drop the `"."` self-member). | ADR §6.1 row 1. |
| `aterm-core/src/lib.rs` | Delete the 3 module decls at lines 56-61 (`pub mod renderer;`, `pub mod renderer_atlas;`, `pub mod renderer_glyph;` and their `#[cfg(feature = "wgpu")]` attrs). Delete the 42 `#[cfg(feature = "wgpu")]` blocks (verified count via `rg -c 'cfg(feature = "wgpu")'`; ADR §6.1 estimated ~30, actual 42 — implementer must remove **all 42**, not "approximately"). Delete the `use crate::renderer::{ColorScheme, TerminalGridRenderer, TerminalThemeMode};` import at line 75 (gated on wgpu, dead). | ADR §6.1 rows 5, 6. |
| `aterm-structure-map.md` (root) | **CREATE** (file is untracked at HEAD `87b0c62` — `git cat-file -e HEAD:aterm-structure-map.md` fails; the local working-tree copy is WIP and NOT authoritative). Author a fresh structure map reflecting post-cleanup truth. Specifically: NO mention of `wgpu`, `glyphon`, `renderer.rs`, `TerminalGridRenderer`, `[[bin]] aterm-v3`, or any `archived/` paths. Include "Build pipeline" section describing `make rust && make swift && make metal && make app`. R2 Option B chosen (see Chunk 6 Task 6.3). | ADR §6.1 row 12 (architect §5 cleanup pre-req). |
| `Cargo.lock` (root) | **REGENERATE** (R2 Option A). Current lockfile contains stale entries `aterm-v3`, `wgpu`, `glyphon`, `winit`, `pollster` (verified 2026-05-08). After Chunk 2 root manifest rewrite, `cargo check --workspace` re-resolves and updates the lockfile. Each chunk that triggers lockfile diff commits the lockfile change as part of the same task commit (see invariant I9 below). `--locked` flag is NOT used in Phase 1 because the lockfile is currently invalid; it becomes valid after Chunk 2. | R2-3 fix (codex review §7 MAJOR). |

**Files to NOT TOUCH (out of scope — Phase 2 territory or unrelated):**

- `aterm-core/src/app.rs` — Phase 2 territory (`SessionAction::AttachExternal` dispatch lives here).
- `aterm-core/src/inject.rs`, `pty.rs`, `session.rs`, `terminal.rs`, `mailbox/`, `tailscale.rs`, `telepty_bridge.rs`, `telepty.rs`, `sync.rs` — runtime, untouched.
- `bin/aterm.js`, `bin/aterm`, `bin/log-monitor.sh`, `bin/version-check.sh` — npm-distributed launcher; `inject` alias work is Phase 2.
- `bin/run-debug.sh` — **R2-2 Option B deferral**. Verified 2026-05-08: lines 14-18 still build `--bin aterm-v3` (the ghost being removed in Chunk 2). The script is already broken (Cargo.toml will lose `[[bin]] aterm-v3`, so `cargo build --bin aterm-v3` fails after Chunk 2). It is NOT in ADR §6.1's deletion list; out of Phase 1 scope per invariant I10. Follow-up cleanup is a separate task. Implementer must NOT delete or edit it in this plan.
- `npm/`, `macos/`, `Makefile`, `build.rs`, `prototype/`, `dist/`, `out/`, `docs/`, `bin/` — kept as-is.
- `scripts/bundle-server.js`, `scripts/generate-app-icons.py`, `scripts/package-aterm-v3-app.sh` — codex C5 verified they do **not** reference `@xterm`. They are NOT in ADR §6.1's deletion list. Leave them. (If a future cleanup wants to re-evaluate these, that is a separate ADR.)
- `jsconfig.json` (root) — not in ADR §6.1. Leave it. (If empirically Tauri/Svelte-only, future cleanup.)
- `node_modules/winit` (referenced as `path = "../winit"` in some places? — no, that path reference is only in root Cargo.toml which we're rewriting; verify by `rg 'path = "../winit"'` returns 0 hits before commit).

---

## Chunk 1: Pre-flight

**Goal:** Lock in a known-green baseline + isolation branch so every deletion in chunks 2–5 has a clean revert target.

### Task 1.1: Verify ADR commit pinning

**Files:** none (read-only verification).

- [ ] **Step 1: Verify ADR commit-SHA is `137aa96` and status is `accepted`**

```bash
cd ~/projects/aigentry-orchestrator
git log --oneline -1 137aa96
grep -A1 'status' docs/adr/2026-05-06-aterm-session-control-opt-3-prime.md | head -4
```
Expected: commit subject `adr(aterm-session-control): status flip proposed → accepted (r4 user signoff)` and frontmatter `status: accepted`. If either fails, **abort the plan** and inject the orchestrator: `BLOCKER: ADR baseline mismatch | expected=137aa96 status=accepted | got=<actual>`.

- [ ] **Step 2: Print the ADR §6.1 file allowlist for visual cross-check**

```bash
cd ~/projects/aigentry-orchestrator
sed -n '250,270p' docs/adr/2026-05-06-aterm-session-control-opt-3-prime.md
```
Expected: file table matching this plan's "Files to DELETE" + "Files to MODIFY". If any row is missing in this plan or extra, **revise the plan first**.

### Task 1.2: Establish working branch

**Files:** none (git only).

> **R3-C1 fix (codex r2 focused review §4 C1):** the local working-tree copy of `aterm-structure-map.md` is known-untracked at HEAD `87b0c62` (R2-4 deferral; CREATE-fresh in Chunk 6 Task 6.3). If left in place it would false-positive Step 2's clean-tree gate below (since the gate requires `git status --short` to be empty). r3 adds a new pre-flight Step 1 that pre-relocates **only that one known file** to `/tmp/`, leaving every other untracked path visible to the gate. Task 6.3 Step 2 (which also relocates the WIP) remains in place and becomes a harmless no-op when the file has already been moved here.
>
> If, post r3, a similar known-untracked WIP needs to be added in a future revision, extend Step 1 below — do NOT broaden the Step 2 gate to "ignore-all-untracked" (that would defeat its purpose).

- [ ] **Step 1: Pre-flight WIP relocation (R3-C1 — known-untracked `aterm-structure-map.md` only)**

```bash
cd ~/projects/aigentry-aterm
# Move the known-untracked WIP aside ONLY if it is untracked (defense-in-depth: never
# relocate a tracked file). Any other untracked path remains visible to Step 2's gate.
if [ -f aterm-structure-map.md ] && ! git ls-files --error-unmatch aterm-structure-map.md >/dev/null 2>&1; then
  mv aterm-structure-map.md /tmp/aterm-structure-map-pre-r2-wip.md
  echo "relocated WIP to /tmp/aterm-structure-map-pre-r2-wip.md"
elif [ -f aterm-structure-map.md ]; then
  echo "ABORT: aterm-structure-map.md is TRACKED — R2-4 / R3-C1 premise broken; inject orchestrator before continuing"
  exit 1
else
  echo "no aterm-structure-map.md in working tree — nothing to relocate"
fi
```
Expected: one of the two non-abort echoes. The abort branch is the same as Task 6.3 Step 1's `TRACKED — STOP` path: an out-of-band commit landed the file, R2-4 Option B / R3-C1 premise invalid, must inject orchestrator before continuing the plan. After this step, the only legitimate dirty path remaining is **none** (clean tree expected at Step 2).

- [ ] **Step 2: Confirm clean working tree**

```bash
cd ~/projects/aigentry-aterm
git status --short
```
Expected: empty output. If dirty, the implementer must stash or commit first **and inject the orchestrator** noting the unrelated WIP — never fold unrelated changes into this cleanup. (The known-untracked `aterm-structure-map.md` was relocated by Step 1 above and must NOT appear here; if it does, Step 1 didn't run or failed silently — re-run it.)

- [ ] **Step 3: Create cleanup branch**

```bash
cd ~/projects/aigentry-aterm
git switch -c phase1/cleanup-2026-05-06
```
Expected: `Switched to a new branch 'phase1/cleanup-2026-05-06'`.

- [ ] **Step 4: Tag the pre-cleanup baseline for cheap rollback**

```bash
cd ~/projects/aigentry-aterm
git tag phase1-cleanup-baseline
```
Expected: tag created (`git tag --list phase1-cleanup-baseline` shows the tag). This tag is the "git revert all the way" anchor.

### Task 1.3: Establish baseline test/build green (active crates only — R2-1)

**Files:** none (build only).

> **R2-1 BLOCKER fix (codex review §5):** the pre-cleanup workspace baseline CANNOT use `cargo check --workspace` / `cargo test --workspace` because root `Cargo.toml` declares `members = [".", ...]` + `[[bin]] aterm-v3` referencing a missing `src-v3/main.rs`. The workspace baseline is invalid until Chunk 2 removes the root ghost. Therefore Chunk 1 baselines target **active sub-crates only** (`-p aterm-core -p aterm-session -p aterm-ipc`). The full-workspace gate moves to **Chunk 2 Task 2.2 Step 4** (post-ghost-removal) and Chunk 6 final.

- [ ] **Step 1: Cargo per-package check (active crates, NOT --workspace)**

```bash
cd ~/projects/aigentry-aterm
cargo check -p aterm-core -p aterm-session -p aterm-ipc 2>&1 | tail -5
```
Expected: `Finished` line (no errors). Warnings about unused `renderer` modules are **expected and fine** — they confirm the dead-code premise. **Do NOT run `cargo check --workspace` here** — it will fail on the root ghost `[[bin]] aterm-v3` pointing at missing `src-v3/main.rs`. The workspace gate is deferred to post-Chunk-2.

- [ ] **Step 2: Cargo per-package test (baseline, active crates only)**

```bash
cd ~/projects/aigentry-aterm
cargo test -p aterm-core -p aterm-session -p aterm-ipc 2>&1 | tee /tmp/aterm-baseline-test.log | tail -20
```
Expected: PASS / FAIL counts captured for the 3 active crates. Per architect deep-analysis (commit `8aeb21e`) the baseline is **70/72 PASS**; the 2 known regressions are FFI panic guard and **independent** of cleanup surface (#363). Record exact counts; the comparison invariant is "Chunk 2 Task 2.2 Step 4 (`cargo test --workspace`) ≥ this active-crates result" and "Chunk 6 final result == Chunk 2 Task 2.2 Step 4 result".

- [ ] **Step 3: npm test baseline (all 4 packages — R2-8)**

```bash
cd ~/projects/aigentry-aterm/npm/aterm && npm test 2>&1 | tail -10 || echo "no test script"
cd ~/projects/aigentry-aterm/npm/aterm-darwin-arm64 && npm test 2>&1 | tail -10 || echo "no test script"
cd ~/projects/aigentry-aterm/npm/aterm-darwin-x64 && npm test 2>&1 | tail -10 || echo "no test script"
cd ~/projects/aigentry-aterm/npm/aterm-linux-arm64 && npm test 2>&1 | tail -10 || echo "no test script"
```
Expected: either explicit PASS or `no test script` per package (launchers commonly have no test script — that's fine; the baseline becomes "still no test script" in Chunk 6). All 4 packages baselined here so Chunk 6's final matches symmetrically (R2-8 fix for codex review §8 npm baseline/final mismatch).

- [ ] **Step 4: Build .app baseline**

```bash
cd ~/projects/aigentry-aterm
make app 2>&1 | tail -20
ls -la build/aterm.app/Contents/MacOS/aterm
stat -f '%z' build/aterm.app/Contents/MacOS/aterm
```
Expected: `make app` succeeds, `build/aterm.app/Contents/MacOS/aterm` exists. Record the binary's size (`stat -f %z`) **into the Task 1.4 commit body** (R2-7 — no separate metrics file). Chunk 6 compares against this value (post-cleanup binary should be **identical or near-identical**, since none of the deletions reach the active build).

### Task 1.4: Anchor baseline commit (metrics in commit body — R2-7)

**Files:** none (no source/data files created — R2-7 Option A: metrics live in commit body, not a separate file).

> **R2-7 fix (codex review §9 #7):** r1 created `docs/superpowers/plans/2026-05-06-aterm-phase1-cleanup-baseline.txt` outside ADR §6.1 and outside this plan's File Structure allowlist. r2 moves the metrics into the commit message body so the audit trail is preserved without scope creep.

- [ ] **Step 1: Capture baseline metrics into a tmp file (commit-body source, NOT a tracked artifact)**

```bash
cd ~/projects/aigentry-aterm
{
  printf 'chore(phase1-cleanup): anchor baseline (Chunk 1 boundary)\n\n'
  printf 'No file changes. Pure anchor commit recording pre-cleanup metrics.\n\n'
  printf -- '--- baseline metadata ---\n'
  printf 'ADR commit: 137aa96\n'
  printf 'Branch: %s\n' "$(git branch --show-current)"
  printf 'Tag: phase1-cleanup-baseline\n'
  printf -- '--- cargo test (active crates) ---\n'
  tail -10 /tmp/aterm-baseline-test.log
  printf -- '\n--- aterm binary size (bytes) ---\n'
  stat -f '%z' build/aterm.app/Contents/MacOS/aterm 2>/dev/null
  printf -- '\n--- archived/ size ---\n'
  du -sh archived/ 2>/dev/null
  printf -- '\n--- aterm-core dead-renderer LOC ---\n'
  wc -l aterm-core/src/renderer.rs aterm-core/src/renderer_atlas.rs aterm-core/src/renderer_glyph.rs
  printf -- '\n--- cfg(feature=wgpu) positive sites ---\n'
  grep -c 'cfg(feature = "wgpu")' aterm-core/src/lib.rs
  printf -- '\n--- cfg(not(feature=wgpu)) negative sites (R2-5: out of Phase 1 scope) ---\n'
  grep -c 'cfg(not(feature = "wgpu"))' aterm-core/src/lib.rs
} > /tmp/aterm-phase1-baseline-commit-msg.txt
cat /tmp/aterm-phase1-baseline-commit-msg.txt
```
Expected: tmp file populated. Dead-renderer LOC line sums to ≥3621 (per ADR), positive `wgpu` feature gates = **42**, negative = **28** (verified 2026-05-08; implementer updates plan if counts differ).

- [ ] **Step 2: Anchor commit using --allow-empty + commit-body metrics**

```bash
cd ~/projects/aigentry-aterm
git commit --allow-empty -F /tmp/aterm-phase1-baseline-commit-msg.txt
rm -f /tmp/aterm-phase1-baseline-commit-msg.txt
```
Expected: empty commit (no file diff, message body carries baseline metrics). This is the **anchor commit** of the Chunk 1 boundary; `git log --format=%B` recovers the metrics for Chunk 6 comparison. The `--allow-empty` flag is justified because R2-7 chose audit-trail-via-commit-body over scope-creep tracked artifacts.

---

## Chunk 2: Ghost Crate Cleanup (root Cargo.toml)

**Goal:** Remove `[[bin]] aterm-v3` (which references a missing `src-v3/main.rs`) and the orphaned root-level wgpu/glyphon/winit/pollster deps. Reduces the root `Cargo.toml` to a workspace-only manifest.

### Task 2.1: Verify the ghost is truly a ghost

**Files:** none (read-only).

- [ ] **Step 1: Confirm `src-v3/main.rs` is missing**

```bash
cd ~/projects/aigentry-aterm
ls -la src-v3/ 2>&1 | head -5
test -f src-v3/main.rs && echo "EXISTS — ABORT" || echo "MISSING — confirmed ghost"
```
Expected: `MISSING — confirmed ghost`. If `EXISTS — ABORT`, the implementer must STOP and inject the orchestrator: `BLOCKER: src-v3/main.rs unexpectedly exists | plan assumption invalid | request architect review`.

- [ ] **Step 2: Verify no consumer references `aterm-v3` binary or root crate library (R2-2 scoped scan)**

```bash
cd ~/projects/aigentry-aterm
rg -n 'aterm[-_]v3' --hidden \
  -g '!archived' -g '!node_modules' -g '!target' -g '!.git' \
  -g '!docs/**' -g '!*.md' -g '!Cargo.lock' \
  -g '!bin/run-debug.sh' -g '!scripts/package-aterm-v3-app.sh'
```
Expected output (R2-2 Option B): exactly the matches in `Cargo.toml` (the `[[bin]]` we're deleting). The plan doc itself, all `docs/**` files, `aterm-structure-map.md`, and `Cargo.lock` are excluded so self-doc/lockfile false positives do NOT trip this gate. The two stale shell consumers (`bin/run-debug.sh`, `scripts/package-aterm-v3-app.sh`) are excluded per **invariant I10** (already-broken, deferred to follow-up; codex review §9 #2). Any match outside `Cargo.toml` → **stop chunk 2** and inject the orchestrator: `BLOCKER: chunk 2 task 2.1 — unexpected aterm-v3 consumer at <path>`. The Makefile builds `aterm-core` not `aterm-v3` — confirm by `grep -n aterm-v3 Makefile` returning empty (Makefile is not in the exclude list, so it must be clean).

- [ ] **Step 2b: Independently confirm the deferred stale consumers still match the I10 fingerprint (defense in depth)**

```bash
cd ~/projects/aigentry-aterm
rg -n 'aterm[-_]v3|src-v3|src-tauri' bin/run-debug.sh scripts/package-aterm-v3-app.sh
```
Expected: matches present (these scripts are still broken — that is the I10 deferral premise). If either script is now CLEAN of these references (e.g., someone fixed them), the I10 deferral note is stale; **stop** and inject the orchestrator: `INFO: chunk 2 task 2.1 step 2b — stale consumer cleanup happened out-of-band; I10 deferral may be obsolete`.

- [ ] **Step 3: Verify root deps are not transitively required by sub-crates**

```bash
cd ~/projects/aigentry-aterm
rg -n '(^|\b)(wgpu|glyphon|winit|pollster)\s*=' aterm-core/Cargo.toml aterm-session/Cargo.toml aterm-ipc/Cargo.toml
```
Expected: no matches in any sub-crate. (Already verified at plan time: `aterm-core/Cargo.toml` does not have `wgpu`/`glyphon`/`winit`/`pollster` and has no `[features]` block.) If any match appears, the implementer must **stop chunk 2** and re-scope: those deps must move into the relevant sub-crate, not be deleted.

- [ ] **Step 4: Verify no `path = "../winit"` references survive the cleanup (R2-2 scoped scan)**

```bash
cd ~/projects/aigentry-aterm
rg -n 'path = "\.\./winit"' --hidden \
  -g '!archived' -g '!node_modules' -g '!target' -g '!.git' \
  -g '!docs/**' -g '!*.md' -g '!Cargo.lock'
```
Expected: matches only in root `Cargo.toml` (the line we're deleting) — typically line 24. The plan doc, ADR, and lockfile are excluded so self-references in markdown don't false-positive. If any other consumer references the local `../winit` path, **stop** and inject the orchestrator: removing the root `winit` dep would break that consumer.

### Task 2.2: Rewrite root Cargo.toml

**Files:**
- Modify: `Cargo.toml` (root)

- [ ] **Step 1: Replace root Cargo.toml with workspace-only manifest**

The post-rewrite manifest MUST contain **only** the `[workspace]` table. The implementer follows these rules verbatim — these are structural assertions, not a copy-pasteable block:

| Section | Action |
|---|---|
| `[workspace]` table | Retain. |
| `members = [".", "aterm-core", "aterm-session", "aterm-ipc"]` | Trim to `members = ["aterm-core", "aterm-session", "aterm-ipc"]` (drop the `"."` self-member because the workspace root no longer carries source). |
| `resolver = "X"` | **Preserve verbatim** if present (whether `"1"` or `"2"`). Do NOT add it if absent — adding `resolver = "2"` is a **behavior change** (feature unification differs between v1↔v2) and violates I1. |
| `[package]` block (the entire block, lines starting at `[package]` and continuing through `publish = false`) | Delete. |
| `[[bin]] name = "aterm-v3" path = "src-v3/main.rs"` | Delete. |
| `[dependencies]` block (the entire block) | Delete. |
| `[target.'cfg(target_os = "macos")'.dependencies]` block | Delete. |
| Any other `[workspace.dependencies]`, `[patch.crates-io]`, `[profile.*]` sections | Preserve verbatim if present. (At plan-write time none are present, but the implementer must not silently drop them.) |

Result: a Cargo.toml file whose only top-level table is `[workspace]` (plus optionally `[patch.*]` / `[profile.*]` / `[workspace.*]` if they existed before).

- [ ] **Step 2: Verify the diff is purely structural deletion + workspace member trim**

```bash
cd ~/projects/aigentry-aterm
git diff Cargo.toml
```
Expected diff (structural assertion, not line-range):
- **Removed:** the `[package]` block, the `[[bin]]` block, the `[dependencies]` block, the `[target.'cfg(target_os = "macos")'.dependencies]` block.
- **Modified:** `members = [...]` trimmed by exactly one entry (`"."` removed).
- **Retained:** `[workspace]` header, any pre-existing `resolver = "..."`, any other workspace-only sections.
- **Added:** zero new dep entries (any `+ … = "..."` line is a reject signal).

If the diff includes anything outside this allowlist, run `git restore Cargo.toml` and re-do Step 1.

- [ ] **Step 3: Cargo workspace check (R2-1 — first valid `--workspace` gate)**

```bash
cd ~/projects/aigentry-aterm
cargo check --workspace 2>&1 | tail -5
```
Expected: `Finished`. **This is the first chunk where `cargo check --workspace` is valid** because root ghost is gone and `members = [".", ...]` is trimmed. If the unused `renderer.rs` modules now error (because they previously inherited wgpu via the root crate's deps), that is **expected** — chunk 3 deletes them. If errors are about anything else (e.g., a sub-crate suddenly missing a transitive dep), **rollback recipe:** `git restore Cargo.toml Cargo.lock`, then re-scope.

- [ ] **Step 4: Cargo workspace test (record delta — first valid `--workspace test`)**

```bash
cd ~/projects/aigentry-aterm
cargo test --workspace 2>&1 | tee /tmp/aterm-postchunk2-workspace-test.log | tail -20
```
Expected: PASS/FAIL counts ≥ Chunk 1 Task 1.3 Step 2 (active-crates-only baseline) — `--workspace` may add tests previously not in the active-crates baseline because Chunk 1 couldn't run `--workspace` (R2-1). Record this number — it becomes the **canonical baseline** for Chunk 6 final comparison ("Chunk 6 result == this Step 4 result"). Any **regression vs Chunk 1 active-crates result** → **rollback recipe:** `git restore Cargo.toml Cargo.lock` and re-scope.

- [ ] **Step 5: Build .app**

```bash
cd ~/projects/aigentry-aterm
make app 2>&1 | tail -10
```
Expected: build succeeds, no regression. On failure: `git restore Cargo.toml Cargo.lock`.

- [ ] **Step 6: Inspect Cargo.lock diff (R2-3 expected lockfile changes)**

```bash
cd ~/projects/aigentry-aterm
git diff --stat Cargo.lock
git diff Cargo.lock | grep -E '^[-+]name = ' | head -20
```
Expected: Cargo.lock changed. Removed entries should include `aterm-v3`, and dependent transitive entries (`wgpu`, `glyphon`, `winit`, `pollster` and their deps) may also drop **if** no other crate brought them in. If the `-` lines do NOT include `aterm-v3`, the manifest edit was incomplete — re-do Step 1. If `+` lines add brand-new entries, that is a regression (no new deps in this plan, I8) — **rollback recipe:** `git restore Cargo.toml Cargo.lock` and re-scope.

- [ ] **Step 7: Commit (Cargo.toml + Cargo.lock paired — I9)**

```bash
cd ~/projects/aigentry-aterm
git add Cargo.toml Cargo.lock
git commit -m "chore(phase1-cleanup): drop ghost [[bin]] aterm-v3 + orphan root deps + lockfile prune (ADR §6.1 row 1, R2-3)"
```
Expected: two-file commit (Cargo.toml + Cargo.lock). Per **invariant I9**, lockfile drift MUST land in the same commit as the manifest change that triggered it — never standalone, never deferred to a later chunk.

### Task 2.3: Delete the empty src-v3/ directory

**Files:**
- Delete: `src-v3/` (empty directory, if present)

- [ ] **Step 1: Confirm src-v3/ is empty (or absent)**

```bash
cd ~/projects/aigentry-aterm
ls -la src-v3/ 2>&1
```
Expected: either "No such file or directory" OR an empty listing (only `.` and `..`). If it has files, **stop** — the plan assumption was wrong; inject the orchestrator.

- [ ] **Step 2: Remove the directory if it exists**

```bash
cd ~/projects/aigentry-aterm
[ -d src-v3 ] && rmdir src-v3 || echo "already absent"
git status --short src-v3 2>/dev/null
```
Expected: directory gone OR `already absent`. If `rmdir` fails because non-empty, abort and inject orchestrator.

- [ ] **Step 3: Post-rmdir build verification**

```bash
cd ~/projects/aigentry-aterm
cargo check --workspace 2>&1 | tail -3
make app 2>&1 | tail -5
```
Expected: green. If anything broke (e.g., `build.rs` or include glob secretly referenced `src-v3/`), recreate the empty dir (`mkdir src-v3 && touch src-v3/.gitkeep`) and inject orchestrator.

- [ ] **Step 4: If git tracked the empty dir (e.g., a `.gitkeep`), commit the removal**

```bash
cd ~/projects/aigentry-aterm
git status --short | grep src-v3 || echo "nothing tracked under src-v3"
# If output shows tracked deletion:
git add -u src-v3 && git commit -m "chore(phase1-cleanup): remove empty src-v3/ directory"
```
Expected: either skip (nothing tracked) or single deletion commit.

---

## Chunk 3: Dead wgpu Renderer Removal (positive gates only — R2-5 Option A)

**Goal:** Delete the 3621 LOC unreachable wgpu renderer trio + the **42 positive `#[cfg(feature = "wgpu")]` blocks** in `aterm-core/src/lib.rs`. **NOT in scope (R2-5 Option A — codex review §9 #5):**
- The **28 negative `#[cfg(not(feature = "wgpu"))]` gates** remain.
- The `no_wgpu_*` named items remain.

These residual references are still **reachable when** `wgpu` feature is undefined (i.e., always, since `aterm-core/Cargo.toml` has no `[features]` block) — they are the **active code path**, not dead code. Deleting them would be a behavior change (I1 violation) and is out of ADR §6.1 scope.

The post-Chunk-3 state therefore has:
- Zero `wgpu::` / `glyphon::` symbol references (the trio + positive gates are gone).
- Residual `cfg(not(feature = "wgpu"))` wrappers around the active no-wgpu code path (kept; these are the live code).
- Residual `no_wgpu_*` names (kept).

A follow-up cleanup (separate task, separate ADR if needed) may un-gate `cfg(not(feature = "wgpu"))` and rename `no_wgpu_*` once `wgpu` is removed from `Cargo.toml` workspace + lockfile entirely. **That cleanup is NOT part of Phase 1.**

### Task 3.1: Verify the renderer trio has zero live consumers

**Files:** none (read-only).

- [ ] **Step 1: Confirm the wgpu feature is undefined in aterm-core**

```bash
cd ~/projects/aigentry-aterm
grep -A5 '\[features\]' aterm-core/Cargo.toml || echo "no [features] block — wgpu feature is undefined, gates are dead"
```
Expected: "no [features] block — wgpu feature is undefined, gates are dead". This is the proof of unreachability.

- [ ] **Step 2: Confirm renderer/atlas/glyph have no consumer outside the cfg gates**

```bash
cd ~/projects/aigentry-aterm
rg -n 'use crate::renderer|use crate::renderer_atlas|use crate::renderer_glyph|use aterm_core::renderer' aterm-core/ aterm-session/ aterm-ipc/ macos/ bin/
```
Expected: every match is either inside the file we're deleting or inside a `#[cfg(feature = "wgpu")]` block in `lib.rs`. The Swift side (`macos/`) must have **zero** matches — Swift renders via Metal (per Makefile lines 47-52), not via the Rust wgpu modules. If a non-gated consumer exists outside the deletion targets, **stop** and inject orchestrator.

- [ ] **Step 3: Confirm `wgpu::` / `glyphon::` symbol references live ONLY in deletion-target files or positive cfg gates (R2-5 gate-aware check)**

```bash
cd ~/projects/aigentry-aterm
# Step 3a: list all wgpu::/glyphon:: symbol sites in aterm-core/src/
rg -n 'wgpu::|glyphon::' aterm-core/src/ > /tmp/aterm-wgpu-symbol-sites.txt
wc -l /tmp/aterm-wgpu-symbol-sites.txt
```
Each site must satisfy at least one of:
1. File path is `aterm-core/src/renderer.rs`, `aterm-core/src/renderer_atlas.rs`, or `aterm-core/src/renderer_glyph.rs` (will be deleted in Tasks 3.2-3.4).
2. Site is inside a positive `#[cfg(feature = "wgpu")]` gated block in `aterm-core/src/lib.rs` (will be deleted in Task 3.5).

> The previous `rg | grep -v` form was rejected by codex review §3 (gate-range unaware — `grep -v` on the ATTRIBUTE LINE doesn't span the multi-line gated block). r2 replaces it with **manual inspection of `/tmp/aterm-wgpu-symbol-sites.txt`** because gate-range awareness is not expressible in `rg`/`grep` regex; the implementer reads each line in `aterm-core/src/lib.rs` and confirms the gated parent. If a site does NOT satisfy either condition (i.e., `wgpu::` reference outside renderer trio AND outside a positive cfg gate), **abort chunk 3** and inject orchestrator: `BLOCKER: chunk 3 task 3.1 step 3 — ungated wgpu/glyphon symbol at <path>:<line>`.

```bash
# Step 3b: inspect (read each site, confirm gate scope manually)
cat /tmp/aterm-wgpu-symbol-sites.txt
```

- [ ] **Step 4: Confirm negative gates are out of scope (R2-5 acknowledgement)**

```bash
cd ~/projects/aigentry-aterm
grep -c 'cfg(not(feature = "wgpu"))' aterm-core/src/lib.rs
```
Expected: **28** (verified 2026-05-08). These are the active code path and remain after Chunk 3 (R2-5 Option A). If the count differs from 28, the divergence is informational — record in commit body. **Do NOT delete these gates** in Phase 1.

### Task 3.2: Delete renderer.rs

**Files:**
- Delete: `aterm-core/src/renderer.rs` (2613 LOC)

- [ ] **Step 1: Delete the file**

```bash
cd ~/projects/aigentry-aterm
git rm aterm-core/src/renderer.rs
```
Expected: `rm 'aterm-core/src/renderer.rs'`.

- [ ] **Step 2: Cargo check (expect lib.rs to fail — that's the next task)**

```bash
cd ~/projects/aigentry-aterm
cargo check --workspace 2>&1 | tail -10
```
Expected: error in `aterm-core/src/lib.rs` about `pub mod renderer;` referring to a missing file. **DO NOT commit yet** — this is a deliberate intermediate state. The commit comes after Task 3.5 cleans lib.rs.

> **Why we don't commit per-file:** The three `pub mod renderer*;` decls in lib.rs reference the three files. Removing one file at a time would leave the workspace red. Instead we make all 4 edits (3 file deletes + lib.rs surgery) in a single working-tree state, verify green, and commit once. This is the one place in the plan where one task spans multiple files — see Task 3.5 for the green-commit step.

### Task 3.3: Delete renderer_atlas.rs

**Files:**
- Delete: `aterm-core/src/renderer_atlas.rs` (385 LOC)

- [ ] **Step 1: Delete the file**

```bash
cd ~/projects/aigentry-aterm
git rm aterm-core/src/renderer_atlas.rs
```
Expected: `rm 'aterm-core/src/renderer_atlas.rs'`. Workspace is still red (lib.rs).

### Task 3.4: Delete renderer_glyph.rs

**Files:**
- Delete: `aterm-core/src/renderer_glyph.rs` (623 LOC)

- [ ] **Step 1: Delete the file**

```bash
cd ~/projects/aigentry-aterm
git rm aterm-core/src/renderer_glyph.rs
```
Expected: `rm 'aterm-core/src/renderer_glyph.rs'`. Workspace is still red.

### Task 3.5: Surgically remove all wgpu feature gates from lib.rs

**Files:**
- Modify: `aterm-core/src/lib.rs`

This task is the largest single edit in the plan. Treat each removal carefully — it is **deletion only**, no replacement.

- [ ] **Step 1: Snapshot positive cfg sites only (R2-5 — exclude negative gates from deletion target)**

```bash
cd ~/projects/aigentry-aterm
# Positive sites (deletion targets):
grep -n 'cfg(feature = "wgpu")' aterm-core/src/lib.rs | grep -v 'cfg(not(' > /tmp/aterm-wgpu-gates.txt
wc -l /tmp/aterm-wgpu-gates.txt
# Defensive check for cfg_attr form (architect ruling required if found):
grep -n 'cfg_attr(.*feature = "wgpu"' aterm-core/src/lib.rs > /tmp/aterm-wgpu-cfg-attr.txt
wc -l /tmp/aterm-wgpu-cfg-attr.txt
```
Expected: `/tmp/aterm-wgpu-gates.txt` has **42 lines** (positive bare `cfg` only, verified 2026-05-08). `/tmp/aterm-wgpu-cfg-attr.txt` is **empty**. If either count is unexpected, see the cfg_attr clause below.

If `/tmp/aterm-wgpu-cfg-attr.txt` is non-empty, **stop** and inject orchestrator: `BLOCKER: chunk 3 task 3.5 — cfg_attr form found at lib.rs:<line>; bare cfg-removal rule does not apply unambiguously`. The ADR §6.1 estimate covers bare `cfg` only; `cfg_attr` removal is grammatically different (it modifies an existing item rather than gating it whole) and needs an architect ruling before proceeding.

- [ ] **Step 2: Delete the 3 module decls (lib.rs:56-61)**

The original lines (per plan-write inspection):
```rust
#[cfg(feature = "wgpu")]
pub mod renderer;
#[cfg(feature = "wgpu")]
pub mod renderer_atlas;
#[cfg(feature = "wgpu")]
pub mod renderer_glyph;
```

Delete all 6 lines (3 attrs + 3 decls) in one edit. The block becomes simply absent — no replacement.

- [ ] **Step 3: Delete the gated import (lib.rs:74-75)**

```rust
#[cfg(feature = "wgpu")]
use crate::renderer::{ColorScheme, TerminalGridRenderer, TerminalThemeMode};
```

Delete both lines.

- [ ] **Step 4: Delete the remaining 39 POSITIVE cfg blocks (R2-5 — positive gates only)**

> **R2-5 scope clarification (codex review §3 / §9 #5):** Step 4 deletes only **positive** `#[cfg(feature = "wgpu")]` blocks. The **28 negative `#[cfg(not(feature = "wgpu"))]` gates** are the live no-wgpu code path and remain (Task 3.1 Step 4 already verified the count). Do NOT touch them.

Each remaining **positive** `#[cfg(feature = "wgpu")]` block has the form:

```rust
#[cfg(feature = "wgpu")]
<item or expression>
```

…where `<item>` may be a struct field, an `impl` method, a `match` arm guard, a `let`-binding, or a free-standing function. **Delete the attribute line AND the gated item/expression** as a unit. For a struct field like `#[cfg(feature = "wgpu")] renderer: Option<TerminalGridRenderer>,` delete both lines. For a method, delete the entire method body. For a single-statement expression, delete the statement.

The implementer should iterate through `/tmp/aterm-wgpu-gates.txt` (snapshot from Step 1, positive form only — `cfg_attr` and negative `cfg(not(...))` form aren't included, see Step 1) and apply this rule to each. **Substring-overlap caveat:** `cfg(feature = "wgpu")` is a substring of `cfg(not(feature = "wgpu"))`. The Step 1 snapshot regex `cfg(_attr)?\(.*feature = "wgpu"` matches BOTH forms — the implementer must filter to lines where the parenthesis after `cfg` is followed by `feature = "wgpu"` directly (not `not(feature = "wgpu")`). A safer in-place pre-filter:

```bash
cd ~/projects/aigentry-aterm
grep -n 'cfg(feature = "wgpu")' aterm-core/src/lib.rs > /tmp/aterm-wgpu-positive-only.txt
wc -l /tmp/aterm-wgpu-positive-only.txt   # should equal positive count (~42)
grep -n 'cfg(not(feature = "wgpu"))' aterm-core/src/lib.rs > /tmp/aterm-wgpu-negative-only.txt
wc -l /tmp/aterm-wgpu-negative-only.txt   # should equal 28 — DO NOT delete these
```

After processing, the **positive** count must drop to **zero**, and the **negative** count must remain **28**:

```bash
cd ~/projects/aigentry-aterm
grep -c 'cfg(feature = "wgpu")' aterm-core/src/lib.rs        # positive sites only — literal pattern, does NOT match the negative form `cfg(not(feature = "wgpu"))`
grep -c 'cfg(not(feature = "wgpu"))' aterm-core/src/lib.rs   # negative sites only
# R3-C2 fix (Option B): two separate greps with explicit expected values per form.
# Expected post-cleanup: positive=0 (all positive gates deleted), negative=28 (kept per R2-5).
```
Expected: positive=**0**, negative=**28** (two distinct counts; the previous "all=28" framing assumed substring overlap that does not actually occur with these literal patterns and was wrong post-cleanup — see frontmatter r3 entry C2). Any other combination → re-do Step 4 (you either missed a positive site OR accidentally deleted a negative gate — diff against `/tmp/aterm-wgpu-positive-only.txt` and `/tmp/aterm-wgpu-negative-only.txt` to locate).

> **If you hit a wgpu-gated POSITIVE item that another (non-gated) item depends on:** stop. The dependency means the gate analysis was wrong. Inject the orchestrator: `BLOCKER: chunk 3 task 3.5 — non-gated consumer of wgpu-gated positive item <name> at lib.rs:<line>`. Do NOT bridge the gap by adding stub code (I8 — no new code).

- [ ] **Step 5: Cargo check — expect green**

```bash
cd ~/projects/aigentry-aterm
cargo check --workspace 2>&1 | tail -10
```
Expected: `Finished`. If errors remain, the wgpu-gate removal in Step 4 missed a site or removed a non-gated line. Diff against the snapshot in Step 1 to locate the miss.

- [ ] **Step 6: Cargo workspace test**

```bash
cd ~/projects/aigentry-aterm
cargo test --workspace 2>&1 | tail -20
```
Expected: PASS/FAIL counts **identical** to the **Chunk 2 Task 2.2 Step 4 canonical workspace baseline** (the first valid `cargo test --workspace` snapshot, post-ghost-removal — recorded in `/tmp/aterm-postchunk2-workspace-test.log`). NOT the Chunk 1 active-crates-only baseline — Chunk 1 could not run `--workspace` at all (R2-1), so it is not a meaningful comparison target here (R3-C3). Any divergence → revert chunk 3 entirely (`git checkout HEAD -- aterm-core/`) and re-scope.

- [ ] **Step 7: Build .app**

```bash
cd ~/projects/aigentry-aterm
make app 2>&1 | tail -10
ls -la build/aterm.app/Contents/MacOS/aterm
stat -f '%z' build/aterm.app/Contents/MacOS/aterm
```
Expected: builds. Binary size matches the Chunk 1 baseline within tolerance (the deleted code never compiled into the binary, so the size should be identical or differ only due to release-build determinism noise).

- [ ] **Step 8: Single commit for the full chunk-3 atomic edit (+ Cargo.lock if dropped)**

```bash
cd ~/projects/aigentry-aterm
git add -- aterm-core/src/lib.rs
# R2-3 / I9 lockfile pairing: if Chunk 3 wgpu removal further reduces the lockfile (e.g., some glyphon-only transitive dep), include Cargo.lock in this same commit.
if ! git diff --quiet Cargo.lock; then
  git add Cargo.lock
fi
git status --short  # expect 3 deletions (renderer*.rs) + 1 modification (lib.rs) [+ optional Cargo.lock]
git commit -m "chore(phase1-cleanup): delete dead wgpu renderer trio + lib.rs feature gates (ADR §6.1 rows 2-6, -3621 LOC; R2-3 lockfile paired if applicable)"
```
Expected: a single commit with `4-5 files changed, 0 insertions(+), 3621+ deletions(-)`. This is one of the two documented multi-file-commit exceptions in the plan (see I5-exception): the file deletions and the lib.rs surgery must move together to keep the tree green, and the Cargo.lock pairing follows I9.

---

## Chunk 4: Tauri/Svelte Residue Removal

**Goal:** Remove the v2 Tauri/Svelte stack (root `package.json`, `package-lock.json`, `src/`, `index.html`, `vite.config.js`, `scripts/patch-xterm-wk-ime.mjs`). Verify the published npm/aterm package is **untouched**.

### Task 4.1: Verify residue is orphaned

**Files:** none (read-only).

- [ ] **Step 1: Confirm root package.json has no live consumer**

```bash
cd ~/projects/aigentry-aterm
cat package.json | head -10
rg -n '"name": "aterm"' npm/ 2>/dev/null && echo "WARNING: name collision — re-check"
```
Expected: root package.json's `"name": "aterm"` (private) is distinct from `"@dmsdc-ai/aterm"` in `npm/aterm/package.json`. The published package lives in `npm/aterm/` and depends on `@dmsdc-ai/aigentry-devkit` + `@dmsdc-ai/aigentry-telepty` — none of those are inherited from the root. **No name collision** confirmed.

- [ ] **Step 2: Confirm `scripts/patch-xterm-wk-ime.mjs` only patches removed v2 packages**

```bash
cd ~/projects/aigentry-aterm
head -20 scripts/patch-xterm-wk-ime.mjs | grep -i 'xterm\|version'
```
Expected: references `@xterm/xterm@6.1.0-beta.195`. That package is only in root `package.json` (the v2 stack), not in `npm/aterm/`. Codex C5 verification on the ADR is the binding evidence — implementer can re-confirm with `rg '@xterm' npm/ scripts/bundle-server.js scripts/package-aterm-v3-app.sh` returning empty.

- [ ] **Step 3: Confirm `src/` is Svelte-only**

```bash
cd ~/projects/aigentry-aterm
ls src/ | head -20
file src/App.svelte
```
Expected: `App.svelte`, `app.css`, `main.js`, `components/`, `design/`, `lib/`. No `.rs`, no `.swift`, no shared types. Pure Svelte. Safe to delete.

- [ ] **Step 4: Confirm `index.html` and `vite.config.js` are root-only entrypoints**

```bash
cd ~/projects/aigentry-aterm
head -20 index.html
head -10 vite.config.js
```
Expected: `index.html` references `src/main.js` (the Svelte mount point). `vite.config.js` is the Vite build config. Both bound to the v2 stack.

- [ ] **Step 5: Confirm Makefile and Rust crates make zero reference to these files**

```bash
cd ~/projects/aigentry-aterm
rg -n 'package\.json|index\.html|vite\.config|patch-xterm' Makefile build.rs aterm-core/ aterm-session/ aterm-ipc/ macos/
```
Expected: empty. (`bin/aterm.js` references `npm/aterm/package.json` indirectly, not root.) If any match appears outside Tauri/Svelte residue, abort.

### Task 4.2: Delete root package.json + package-lock.json

**Files:**
- Delete: `package.json`, `package-lock.json`

- [ ] **Step 1: Delete both**

```bash
cd ~/projects/aigentry-aterm
git rm package.json package-lock.json
```

- [ ] **Step 2: Sanity build (Rust + Swift unaffected)**

```bash
cd ~/projects/aigentry-aterm
cargo check --workspace 2>&1 | tail -3
make app 2>&1 | tail -10
```
Expected: green. The Rust/Swift build path never touched root `package.json`.

- [ ] **Step 3: Commit**

```bash
cd ~/projects/aigentry-aterm
git commit -m "chore(phase1-cleanup): remove root package.json + package-lock.json (ADR §6.1 rows 9-10)"
```

### Task 4.3: Delete Tauri/Svelte source files

**Files:**
- Delete: `src/` (entire dir), `index.html`, `vite.config.js`

- [ ] **Step 1: Delete src/ recursively**

```bash
cd ~/projects/aigentry-aterm
git rm -r src/
```
Expected: deletion of `App.svelte`, `app.css`, `main.js`, `components/*`, `design/*`, `lib/*`. Implementer verifies the count with `git status --short | wc -l`.

- [ ] **Step 2: Delete index.html + vite.config.js**

```bash
cd ~/projects/aigentry-aterm
git rm index.html vite.config.js
```

- [ ] **Step 3: Sanity build**

```bash
cd ~/projects/aigentry-aterm
cargo check --workspace 2>&1 | tail -3
make app 2>&1 | tail -10
```
Expected: green.

- [ ] **Step 4: Commit**

```bash
cd ~/projects/aigentry-aterm
git commit -m "chore(phase1-cleanup): remove Svelte src/ + index.html + vite.config.js (ADR §6.1 row 9)"
```

### Task 4.4: Delete patch-xterm-wk-ime.mjs

**Files:**
- Delete: `scripts/patch-xterm-wk-ime.mjs`

- [ ] **Step 1: Final verification — consumer scan EXCLUDES the script itself + plan/docs (R2-6)**

```bash
cd ~/projects/aigentry-aterm
rg -n 'patch-xterm-wk-ime' . \
  --glob '!archived/' --glob '!node_modules/' \
  --glob '!docs/**' --glob '!*.md' \
  --glob '!scripts/patch-xterm-wk-ime.mjs'
```
Expected: empty. The script being deleted, this plan doc, the ADR, and `docs/**` are all excluded so they don't false-positive (R2-6 fix for codex review §9 #6 — the previous `rg . --glob '!archived/' --glob '!node_modules/'` would have matched the script itself + every doc reference). After Task 4.2 deleted root `package.json`, the only legitimate `postinstall` consumer is gone; the only remaining match would be the script's own filename in scripts/ which is now excluded. If non-empty, a real consumer exists — **abort** and inject orchestrator.

- [ ] **Step 2: Delete + commit**

```bash
cd ~/projects/aigentry-aterm
git rm scripts/patch-xterm-wk-ime.mjs
cargo check --workspace 2>&1 | tail -3
make app 2>&1 | tail -5
git commit -m "chore(phase1-cleanup): remove orphaned scripts/patch-xterm-wk-ime.mjs (ADR §6.1 row 11, codex C5)"
```
Expected: green build + single-file commit.

### Task 4.5: Clean up node_modules/ working tree

**Files:** none (working tree only — gitignored).

- [ ] **Step 1: Confirm node_modules/ is gitignored**

```bash
cd ~/projects/aigentry-aterm
git check-ignore -v node_modules/
```
Expected: a `.gitignore` rule reports the ignore. If not gitignored, **abort** — adding it to `.gitignore` is a separate concern; do not bundle.

- [ ] **Step 2: Remove the working-tree directory**

```bash
cd ~/projects/aigentry-aterm
rm -rf node_modules/
ls -la node_modules/ 2>&1 | head -3
```
Expected: "No such file or directory". No commit needed (gitignored).

### Task 4.6: Verify npm/aterm is untouched

**Files:** none (read-only).

- [ ] **Step 1: Verify npm package.json + bin/aterm.js are intact**

```bash
cd ~/projects/aigentry-aterm
diff <(git show phase1-cleanup-baseline:npm/aterm/package.json) npm/aterm/package.json
diff <(git show phase1-cleanup-baseline:bin/aterm.js) bin/aterm.js
```
Expected: both diffs empty. If anything differs, **revert** that file from the tag.

- [ ] **Step 2: npm/aterm test (matches Chunk 1 baseline)**

```bash
cd ~/projects/aigentry-aterm/npm/aterm && npm test 2>&1 | tail -10 || echo "no test script (matches baseline)"
```
Expected: same outcome as Chunk 1 Task 1.3 Step 3.

---

## Chunk 5: archived/ Purge

**Goal:** Remove the local `archived/` working-tree directory (~5.1 GB). Git history retains it (Lesson F3).

### Task 5.1: Verify archived/ has no live consumer

**Files:** none (read-only).

- [ ] **Step 1: Enumerate archived/ subdirs and capture for the commit body**

```bash
cd ~/projects/aigentry-aterm
ls -la archived/ | tee /tmp/aterm-archived-listing.txt
du -sh archived/* 2>/dev/null | sort -h | tee -a /tmp/aterm-archived-listing.txt
```
Expected: at least `src-v3-future/`, `src-tauri-v1/`, `.DS_Store`. Record the total size — chunk 6 will compare. **Sweep gate:** every entry in `/tmp/aterm-archived-listing.txt` is by definition obsolete (the entire `archived/` tree is dead per ADR §6.1 rows 7-8 + plan File Structure "any subdir under archived/ is by definition obsolete"). The listing is appended to the chunk-5 commit message body so the audit trail records exactly what was removed.

- [ ] **Step 2: Confirm no source/build references reach into archived/**

```bash
cd ~/projects/aigentry-aterm
rg -n 'archived/' --glob '!archived/**' --glob '!node_modules/**' --glob '!*.md'
```
Expected: empty (or only matches in `*.md` files anywhere, which are excluded by `--glob '!*.md'`). The `*.md` exclusion intentionally allows architectural notes to reference `archived/` historically; those references stay valid because git history preserves the contents. **Coverage note:** this scan covers non-markdown files inside `docs/` too (e.g., `docs/**/*.json`, `docs/**/*.yaml`), since the previous broader `--glob '!docs/**'` would have hidden those.

- [ ] **Step 3: Confirm git history retains archived/**

```bash
cd ~/projects/aigentry-aterm
git log --oneline -1 -- archived/src-v3-future/ archived/src-tauri-v1/
git ls-tree phase1-cleanup-baseline -- archived/src-v3-future/ archived/src-tauri-v1/ | head -10
```
Expected: at least one commit referencing the path AND the `git ls-tree` lists tree entries under both archived subdirs. This is the proof that post-deletion the history-anchored audit trail still resolves via `git log` / `git ls-tree <sha> -- archived/...` / `git show <sha>:archived/<known-file>` (note: `git show <sha>:archived/<dir>/` on a directory errors — use `git ls-tree` for trees and reserve `git show` for specific blobs).

### Task 5.2: Delete archived/

**Files:**
- Delete: `archived/` (entire dir)

- [ ] **Step 1: Remove via git**

```bash
cd ~/projects/aigentry-aterm
git rm -rf archived/
git status --short | head -5
```
Expected: many `D` lines for archived contents. This is one large diff but logically a single conceptual deletion.

- [ ] **Step 2: Sanity build**

```bash
cd ~/projects/aigentry-aterm
cargo check --workspace 2>&1 | tail -3
make app 2>&1 | tail -5
```
Expected: green.

- [ ] **Step 3: Commit (with full archived/ listing in body for audit)**

Use a **file-based** commit message (NOT heredoc with command substitution) so adversarial filenames inside `archived/` cannot trigger shell expansion:

```bash
cd ~/projects/aigentry-aterm
{
  printf 'chore(phase1-cleanup): purge archived/ working tree (~5GB, ADR §6.1 rows 7-8); git history retained\n\n'
  printf 'Removed entries (from /tmp/aterm-archived-listing.txt):\n'
  cat /tmp/aterm-archived-listing.txt
} > /tmp/aterm-archived-commit-msg.txt
git commit -F /tmp/aterm-archived-commit-msg.txt
```
Expected: single commit (potentially with thousands of file deletions); the commit body enumerates exactly what was removed for future audit. The two-stage approach (assemble message in a file, then `git commit -F <file>`) avoids any shell evaluation of listing contents — safer than heredoc with `$(cat ...)`, which would expand backticks / `$()` / unescaped `$VAR` in filenames.

- [ ] **Step 4: Verify history still resolves (primary check via `git ls-tree`)**

```bash
cd ~/projects/aigentry-aterm
git ls-tree phase1-cleanup-baseline -- archived/src-v3-future/ archived/src-tauri-v1/ | head -10
# Optional secondary check on a specific blob (replace <known-file> with an actual file from Step 1's listing):
# git show phase1-cleanup-baseline:archived/src-tauri-v1/Cargo.toml | head -5
```
Expected: `git ls-tree` lists tree entries under both archived subdirs (proof the audit trail is preserved). The optional `git show <sha>:<file>` works only on blobs (specific files), never on trees — `git show <sha>:archived/<dir>/` on a directory will error with `fatal: invalid object`, so do NOT use that form as a primary check.

- [ ] **Step 5: Side-effects note (informational, no action required)**

After deleting 5GB:
- macOS Spotlight reindexes for ~1-2 minutes (cosmetic; does not affect builds).
- IDE workspaces with `archived/` open may show stale buffers — close+reopen affected windows.
- Disk reclaim is immediate (`rm` semantics); `git gc` is **NOT** run (would unnecessarily probe history; I7 prohibits any history-rewrite tooling, and `git gc --prune=now` would be the wrong reflex here).

---

## Chunk 6: Final Verification + Structure-map Refresh

**Goal:** Confirm the working tree is green across all build paths, the LOC delta is real, and `aterm-structure-map.md` no longer references anything that was deleted.

### Task 6.1: Full workspace test

**Files:** none (build only).

- [ ] **Step 1: Cargo workspace test**

```bash
cd ~/projects/aigentry-aterm
cargo test --workspace 2>&1 | tee /tmp/aterm-postcleanup-test.log | tail -20
```
Expected: PASS/FAIL counts **identical** to the **Chunk 2 Task 2.2 Step 4 canonical workspace baseline** (the first valid `cargo test --workspace` snapshot, post-ghost-removal — recorded in `/tmp/aterm-postchunk2-workspace-test.log`). The Chunk 1 Task 1.3 Step 2 baseline was active-crates-only (R2-1) and is NOT a valid `--workspace` comparison target (R3-C3). Any divergence vs the Chunk 2 Step 4 canonical baseline is a regression — file a blocker.

- [ ] **Step 2: npm test (npm/aterm + sub-packages)**

```bash
cd ~/projects/aigentry-aterm/npm/aterm && npm test 2>&1 | tail -10 || echo "no test script"
cd ~/projects/aigentry-aterm/npm/aterm-darwin-arm64 && npm test 2>&1 | tail -10 || echo "no test script"
cd ~/projects/aigentry-aterm/npm/aterm-darwin-x64 && npm test 2>&1 | tail -10 || echo "no test script"
cd ~/projects/aigentry-aterm/npm/aterm-linux-arm64 && npm test 2>&1 | tail -10 || echo "no test script"
```
Expected: same outcome as Chunk 1 baselines.

- [ ] **Step 3: make app — final build**

```bash
cd ~/projects/aigentry-aterm
make app 2>&1 | tail -20
ls -la build/aterm.app/Contents/MacOS/aterm
stat -f '%z' build/aterm.app/Contents/MacOS/aterm
```
Expected: build succeeds. Binary size matches Chunk 1 baseline within build-determinism tolerance.

- [ ] **Step 4: Smoke-test the .app (spawn-and-kill)**

```bash
cd ~/projects/aigentry-aterm
# Spawn the binary, give it 3s to either start cleanly or print a startup error, then kill it.
( build/aterm.app/Contents/MacOS/aterm & echo $! > /tmp/aterm-smoke.pid ) 2>&1 &
sleep 3
SMOKE_PID=$(cat /tmp/aterm-smoke.pid 2>/dev/null)
if kill -0 "$SMOKE_PID" 2>/dev/null; then
  echo "SMOKE PASS: process $SMOKE_PID alive after 3s"
  kill "$SMOKE_PID" 2>/dev/null
else
  echo "SMOKE FAIL: process exited within 3s — investigate"
  exit 1
fi
```
Expected: `SMOKE PASS`. The process must survive 3 seconds without immediate crash. We do **NOT** call `--version` because Chunk 1 did not capture a `--version` baseline, so there is no comparison contract for that flag — adding one in Chunk 6 alone would either be a flag-existence assumption (false-positive risk) or require retroactive edits to Chunk 1. Spawn-and-kill is the cheapest way to detect a regression that would be invisible to `cargo test` (e.g., dylib loading, Metal pipeline init).

### Task 6.2: Compute and record LOC delta (commit body — R2-7)

**Files:** none (no separate `*results.txt` file — R2-7 Option A: metrics live in commit body, paired with Task 6.3 below).

> **R2-7 fix (codex review §9 #7):** r1 created `docs/superpowers/plans/2026-05-06-aterm-phase1-cleanup-results.txt` outside ADR §6.1 and outside this plan's File Structure allowlist. r2 moves the metrics into the commit message body for the Chunk 6 Task 6.3 commit (which is the next file-touching task) so the audit trail is preserved without scope creep.

- [ ] **Step 1: Capture post-cleanup metrics into a tmp file (commit-body source for Task 6.3)**

```bash
cd ~/projects/aigentry-aterm
{
  printf -- '--- Phase 1 Cleanup Results (post-r2 plan execution) ---\n'
  printf 'Branch: %s\n' "$(git branch --show-current)"
  printf 'Baseline tag: phase1-cleanup-baseline\n'
  printf 'Anchor commit (Chunk 1 Task 1.4 baseline): %s\n' "$(git log --format=%H --grep='anchor baseline' -1)"
  printf -- '\n--- LOC delta vs baseline tag ---\n'
  git diff --shortstat phase1-cleanup-baseline..HEAD
  printf -- '\n--- Per-file deletions ---\n'
  git diff --stat phase1-cleanup-baseline..HEAD | tail -25
  printf -- '\n--- Post-cleanup cargo test --workspace ---\n'
  tail -10 /tmp/aterm-postcleanup-test.log
  printf -- '\n--- Cargo.lock prune (R2-3) ---\n'
  printf 'Removed lockfile entries (compare against pre-Chunk-2 state):\n'
  git show "$(git log --format=%H --grep='drop ghost' -1)" -- Cargo.lock | grep '^-name = ' | head -20
  printf -- '\n--- Disk savings (archived/) ---\n'
  du -sh archived/ 2>/dev/null || printf 'archived/ removed: 5.1GB freed\n'
  printf -- '\n--- aterm binary size (bytes) ---\n'
  stat -f '%z' build/aterm.app/Contents/MacOS/aterm
  printf -- '\n--- Residual cfg gates (R2-5 deferred) ---\n'
  printf 'positive cfg(feature="wgpu") sites: '
  grep -c 'cfg(feature = "wgpu")' aterm-core/src/lib.rs 2>/dev/null
  printf 'negative cfg(not(feature="wgpu")) sites (KEPT — Phase 2 follow-up): '
  grep -c 'cfg(not(feature = "wgpu"))' aterm-core/src/lib.rs 2>/dev/null
} > /tmp/aterm-phase1-results-body.txt
cat /tmp/aterm-phase1-results-body.txt
```
Expected: total deletion ≥ 3621 LOC (renderer trio) + ~42 positive cfg lines from lib.rs + ~30 lines from root Cargo.toml + Svelte `src/` (likely ~1000+ LOC) + ~75KB lockfile lines. Net working-tree shrink is >5GB once `archived/` is included. Positive `cfg(feature = "wgpu")` count must be **0**, negative **28** (R2-5 deferred). The tmp file is consumed by Task 6.3 Step 5 commit and discarded — never tracked.

### Task 6.3: CREATE aterm-structure-map.md (R2-4 Option B)

**Files:**
- Create: `aterm-structure-map.md` (root)

> **R2-4 BLOCKER fix (codex review §9 #4):** the file is **untracked at HEAD** `87b0c62` — `git cat-file -e HEAD:aterm-structure-map.md` returns `fatal: path 'aterm-structure-map.md' exists on disk, but not in 'HEAD'`. The local working-tree copy (`16047 bytes`, mtime `2026-04-20`) is WIP and **not authoritative**. r1 told the implementer to "rewrite" — but rewriting an untracked file is ambiguous source state. r2 chooses **Option B: CREATE** the file fresh, reflecting post-cleanup truth, and ignore the local WIP entirely.

- [ ] **Step 1: Confirm the file is still untracked (defense in depth)**

```bash
cd ~/projects/aigentry-aterm
git cat-file -e HEAD:aterm-structure-map.md && echo "TRACKED — STOP, R2-4 deferral premise broken" || echo "untracked — proceed with CREATE"
git ls-files --error-unmatch aterm-structure-map.md 2>&1 | head -3
```
Expected: `untracked — proceed with CREATE`. If `TRACKED — STOP`, an out-of-band commit landed the file; **abort** and inject orchestrator: `INFO: chunk 6 task 6.3 — aterm-structure-map.md tracked out-of-band; R2-4 Option B premise invalid; need architect re-decision (B → A or C?)`.

- [ ] **Step 2: Discard any local WIP (the WIP is not authoritative — r2 ignores it)**

```bash
cd ~/projects/aigentry-aterm
# The pre-existing untracked working-tree copy is WIP and not authoritative.
# R2-4 Option B says we author fresh content. Move the WIP aside (do not delete — preserve in case implementer wants to cross-reference).
[ -f aterm-structure-map.md ] && mv aterm-structure-map.md /tmp/aterm-structure-map-pre-r2-wip.md
ls -la /tmp/aterm-structure-map-pre-r2-wip.md 2>/dev/null
```
Expected: WIP moved to `/tmp/`. Implementer MAY consult it for prior-art ideas, but the new file's content is authored from scratch against the post-cleanup truth (Step 3 checklist).

- [ ] **Step 3: Author `aterm-structure-map.md` from the post-cleanup truth checklist**

Required sections (all sections are mandatory; no architectural claims beyond what the post-cleanup repo state literally proves):

1. **Purpose & status** — one paragraph: "Repository structure map post-Phase-1 cleanup (ADR `~/projects/aigentry-orchestrator/docs/adr/2026-05-06-aterm-session-control-opt-3-prime.md`). Snapshot date: <YYYY-MM-DD>. Source of truth for the next Phase 2 work session."
2. **Cargo workspace layout** — bulleted list: `aterm-core` (lib + cdylib), `aterm-session`, `aterm-ipc`. No root crate. Members declared in `Cargo.toml` (workspace-only manifest after Chunk 2).
3. **Build pipeline** — describe `make rust && make swift && make metal && make app` exactly as `Makefile` lines 24-60 declare. Rendering is **Metal** (Swift). NO mention of `wgpu`, `glyphon`, `renderer.rs`, `TerminalGridRenderer`, `[[bin]] aterm-v3`, `archived/`, `src-v3/`, `src-tauri/`, root `package.json`, `vite`, `index.html`, `App.svelte`, `tauri`.
4. **Distribution** — npm packages: `npm/aterm` (launcher), `npm/aterm-darwin-arm64`, `npm/aterm-darwin-x64`, `npm/aterm-linux-arm64`. The published `npm/aterm/package.json` is the only `package.json` in the repo (root `package.json` deleted in Chunk 4).
5. **Phase 2 entry points** — one paragraph: ADR §6.2 lists the Phase 2 file scope (`app.rs:851-860 SessionAction::AttachExternal`, `telepty_bridge.rs`, `bin/aterm.js inject` alias). Out of scope for this map; cited by reference only.
6. **Known follow-ups (Phase 1 deferrals)** — bullets:
   - 28 `cfg(not(feature = "wgpu"))` gates remain in `aterm-core/src/lib.rs` (R2-5 deferral; un-gate task pending).
   - `bin/run-debug.sh` and `scripts/package-aterm-v3-app.sh` reference the removed `aterm-v3` ghost (R2-2 Option B / I10 deferral; cleanup task `#TBD-aterm-v3-shell-script-cleanup` pending orchestrator backlog registration).

- [ ] **Step 4: Verify zero stale references in the new file**

```bash
cd ~/projects/aigentry-aterm
rg -ni 'wgpu|glyphon|renderer\.rs|renderer_atlas|renderer_glyph|\[\[bin\]\]\s*aterm-v3|aterm-v3\s+binary|src-v3/|archived/|package-lock|index\.html|vite\.config|vite |patch-xterm|App\.svelte|svelte|tauri|node_modules|pollster|^\s*winit\b' aterm-structure-map.md \
  | rg -v 'npm/aterm/package\.json' \
  | grep -vE 'cfg\(not\(feature = "wgpu"\)\)|R2-5|R2-2|aterm-v3 ghost'
```
Expected: empty after the post-filters. The post-filters intentionally allow the **deferral mention** in section 6 (e.g., `cfg(not(feature = "wgpu"))` and `aterm-v3 ghost` are legitimate citations of what is NOT cleaned up). They do NOT permit any **active** description of the deleted surface.

- [ ] **Step 5: Commit (paired with metrics body — R2-7)**

```bash
cd ~/projects/aigentry-aterm
git add aterm-structure-map.md
{
  printf 'docs(phase1-cleanup): create aterm-structure-map.md + record results metrics (R2-4 + R2-7)\n\n'
  cat /tmp/aterm-phase1-results-body.txt
} > /tmp/aterm-structure-map-commit-msg.txt
git commit -F /tmp/aterm-structure-map-commit-msg.txt
rm -f /tmp/aterm-structure-map-commit-msg.txt /tmp/aterm-phase1-results-body.txt
```
Expected: single-file commit (just `aterm-structure-map.md`), with the Task 6.2 metrics in the commit body. The tmp commit-msg files are cleaned up after the commit lands.

### Task 6.4: Memory/CLAUDE.md cross-check (read-only)

**Files:** none (read-only verification).

- [ ] **Step 1: Confirm `feedback_aterm_v3_only.md` is still consistent**

```bash
cat ~/.claude/projects/-Users-duckyoungkim-projects/memory/feedback_aterm_v3_only.md
```
Expected: the memory says "src-v3 순수 Rust만. v2 폐기" — the cleanup makes this **more** true, not less. No memory edit needed unless the wording explicitly references files we just deleted (in which case the implementer reports the discrepancy to the orchestrator; the orchestrator decides whether memory needs an update — the architect/coder does NOT unilaterally rewrite memory).

- [ ] **Step 2: Confirm `feedback_aterm_philosophy.md` is still consistent**

```bash
cat ~/.claude/projects/-Users-duckyoungkim-projects/memory/feedback_aterm_philosophy.md
```
Expected: "경량 크로스 CLI 오케스트레이터 + 터미널. iced 제거. 오버엔지니어링 금지." — the cleanup is a direct embodiment. No edit needed.

### Task 6.5: Final chunk boundary commit (none if no diff)

**Files:** none.

- [ ] **Step 1: Verify working tree is clean**

```bash
cd ~/projects/aigentry-aterm
git status --short
```
Expected: empty. The chunk 6 commits already covered all changes.

---

## Chunk 7: Plan Review + Execution Handoff

**Goal:** Run the plan-document-reviewer subagent loop on this document until APPROVED, then hand off to subagent-driven-development for implementation.

### Task 7.1: Plan-document-reviewer iteration

**Files:** this plan document.

- [ ] **Step 1: Dispatch plan-document-reviewer for chunks 1-3**

Provide the reviewer:
- Path to this plan: `~/projects/aigentry-aterm/docs/superpowers/plans/2026-05-06-aterm-phase1-cleanup.md`
- Path to source spec (binding ADR): `~/projects/aigentry-orchestrator/docs/adr/2026-05-06-aterm-session-control-opt-3-prime.md` (commit `137aa96`)
- Chunk slice for review: chunks 1, 2, 3 (lines from "## Chunk 1: Pre-flight" through end of "## Chunk 3").

If reviewer returns ❌ Issues Found: fix in place, re-dispatch. Max 5 iterations per chunk slice. If iter > 3 without convergence, inject the orchestrator: `BLOCKER: chunk 1-3 review iter > 3 | last verdict: <verdict> | request architect intervention`.

- [ ] **Step 2: Dispatch plan-document-reviewer for chunks 4-6**

Same procedure for chunks 4, 5, 6.

- [ ] **Step 3: Final approval gate**

When all chunks return ✅ Approved, the plan is ready for execution. Add an "approved by plan-document-reviewer" line at the end of this section with the iter count for each slice.

### Task 7.2: Execution handoff

This plan executes via **superpowers:subagent-driven-development** (aterm has the `aigentry-aterm-coder-*` session role available; subagents are present per the harness in this ecosystem).

The orchestrator (NOT the architect) is responsible for:
1. Committing this plan document to `aigentry-aterm` (the architect's `NO commit yet` invariant from the dispatch envelope means the orchestrator commits after architect reports DONE).
2. Dispatching `aigentry-aterm-coder-*` with the subagent-driven-development skill, pointing it at this plan.
3. Monitoring chunk-by-chunk progress via the per-task commit messages.

Architect responsibilities end at: plan committed-by-orchestrator + DONE inject acknowledged.

---

## Reviewer Verdict (filled by plan-document-reviewer loop)

| Chunk slice | Iter count | Final verdict | Notes |
|---|---:|---|---|
| Chunks 1-3 | (TBD) | (TBD) | |
| Chunks 4-6 | (TBD) | (TBD) | |
| Chunk 7 (this section, self-review optional) | n/a | n/a | |

---

## Appendix A: ADR §6.1 ↔ Plan Chunk Mapping (traceability)

| ADR §6.1 row | Source line | Plan chunk / task |
|---|---|---|
| `Cargo.toml` (root): remove `[[bin]] aterm-v3` + unused root deps | line 258 | Chunk 2, Task 2.2 |
| `aterm-core/src/renderer.rs` (delete) | line 259 | Chunk 3, Task 3.2 |
| `aterm-core/src/renderer_atlas.rs` (delete) | line 260 | Chunk 3, Task 3.3 |
| `aterm-core/src/renderer_glyph.rs` (delete) | line 261 | Chunk 3, Task 3.4 |
| `aterm-core/src/lib.rs:51-67` mod decls (delete 3) | line 262 | Chunk 3, Task 3.5 Step 2 |
| `aterm-core/src/lib.rs` 30 cfg sites | line 263 | Chunk 3, Task 3.5 Steps 3-4 (42 positive sites deleted; 28 negative `cfg(not(...))` sites kept per R2-5 Option A) |
| `archived/src-v3-future/` (delete) | line 264 | Chunk 5, Task 5.2 |
| `archived/src-tauri-v1/` (delete) | line 265 | Chunk 5, Task 5.2 |
| Root `package.json`, `src/`, `vite.config.js`, `index.html` (delete) | line 266 | Chunk 4, Tasks 4.2 + 4.3 |
| Root `package-lock.json` (delete) | line 267 | Chunk 4, Task 4.2 |
| `scripts/patch-xterm-wk-ime.mjs` (delete) | line 268 | Chunk 4, Task 4.4 |
| `aterm-structure-map.md` (rewrite) | line 269 | Chunk 6, Task 6.3 |

Every ADR §6.1 row maps 1:1 to a plan task. No row is unmapped.

---

## Appendix B: Lessons Embedded in this Plan

| Lesson (from dispatch envelope) | Where embedded |
|---|---|
| **F1**: Past cleanup deleted files without verifying consumers. | Each delete task has a "Verify no consumer" step using `rg`. |
| **F2**: Dead code "obviously unused" turned out reachable via macro/feature gate. | Chunk 1 establishes a green baseline; every chunk re-runs `cargo check`/`cargo test` before commit. Chunk 3 Task 3.1 Step 1 explicitly verifies the wgpu feature is undefined. |
| **F3**: `archived/` deletion lost git history. | Chunk 5 Task 5.1 Step 3 + Task 5.2 Step 4 verify `git show <baseline>:archived/...` still resolves post-delete. No history rewrite (I7). |
| **F4**: Reviewer flagged "missing test for delete" — interpret as deletion-verification IS the test. | Plan uses "verify deleted + green build" as the deletion test, not a TDD red→green pair. The "tests" are: (a) green `cargo test` baseline preserved, (b) green `make app`, (c) `git show` history resolution. |

---

## Appendix C: Reversibility Map

Every chunk ends on a green commit. Reversibility is **per-task** via `git revert`:

| Chunk | Anchor commit | Revert blast radius |
|---|---|---|
| 1 | "anchor baseline (Chunk 1 boundary)" — empty commit, body carries baseline metrics (R2-7) | revert is harmless (empty commit, just removes audit body) |
| 2 | "drop ghost [[bin]] aterm-v3 + orphan root deps + lockfile prune" — Cargo.toml + Cargo.lock paired (R2-3 / I9) | restores root Cargo.toml `[package]` block AND lockfile state; build still green |
| 3 | "delete dead wgpu renderer trio + lib.rs feature gates" — also pairs Cargo.lock if dropped | restores 3621 LOC + 42 positive cfg gates; build still green (gates were no-ops anyway). Negative gates unaffected (R2-5: never deleted in Phase 1). |
| 4 | three commits (4.2, 4.3, 4.4) | each independently revertible; restores Tauri/Svelte stack |
| 5 | "purge archived/" | restores 5GB; works because git history retained the blobs |
| 6 | "create aterm-structure-map.md + record results metrics" — file create + commit-body metrics (R2-4 + R2-7) | revert removes the new structure map; harmless (file was untracked at baseline anyway) |

Total rollback to baseline: `git reset --hard phase1-cleanup-baseline`. Per ADR §1 this is a **two-way** decision.
