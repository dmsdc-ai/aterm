# Research: Claude Code /buddy feature + aterm Integration

## Overview: Claude Code `/buddy` (2026)

In April 2026, Claude Code introduced a "Tamagotchi-style" virtual pet system known as **Buddy**. This feature gamifies the developer experience by linking a terminal-native companion to coding activity and terminal telemetry.

### Technical Architecture
- **Rendering:** Uses **React + Ink** for TTY-based rendering. It typically lives in the status line or a dedicated "watcher" process.
- **State Management:** Stats and species data are persisted in `~/.claude/buddy.json` or `~/.claude/pets/`.
- **Gacha Logic:** 18 species (Duck, Capybara, Axolotl, etc.) with rarity tiers (Common to Legendary). Species are determined by a **Mulberry32 PRNG** seeded with the user's ID and salt `friend-2026-401`.
- **Core Stats:** 
    - `DEBUGGING`: Increased by running tests/debuggers.
    - `CHAOS`: Increased by rapid uncommitted changes.
    - `SNARK`: Personality trait influencing comments on code quality.
- **Lifecycle:** Energy decays over ~3 days; replenished by "feeding" tokens (1M tokens = +1 energy).

---

## Integration Opportunities for `aterm`

As a GPU-accelerated, native (Swift/Rust) terminal, `aterm` can transcend the limitations of TTY-based ASCII rendering to provide a "First Class" Buddy experience.

### 1. High-Fidelity Native Overlay (Metal)
- **Current Limitation:** Claude Code renders Buddy as ASCII/ANSI in the terminal buffer.
- **`aterm` Opportunity:** Use the **Metal renderer** to overlay high-resolution 2D sprites or even 3D models of the Buddy.
- **Implementation:** A `BuddyOverlayView` in Swift that sits on top of the `TerminalView`. It can animate independently of the text grid.

### 2. Multi-Session "Mascot" (telepty)
- **`aterm` Advantage:** `aterm` already tracks multiple sessions across workspaces.
- **Opportunity:** The Buddy can become a **shared project mascot**. If multiple developers (or multiple agents) are active in a workspace (detected via `aterm list`), the Buddy can "travel" between sessions or appear in the `SessionSidebarView` as a global project entity.

### 3. Native Telemetry Feed
- **Opportunity:** `aterm-core` (Rust) can hook into PTY events (e.g., high frequency of `stderr`, long-running builds) and feed this data directly to the Buddy's `DEBUGGING` and `CHAOS` stats via a background IPC channel, making the Buddy feel more reactive to the *terminal* state rather than just *CLI* commands.

### 4. Safety Net: Native macOS Approval
- **Context:** Experimental Buddy versions include a "Safety Net" that blocks dangerous Claude operations.
- **Opportunity:** Instead of a text prompt, `aterm` can intercept the "block" event and trigger a **native macOS Alert or SwiftUI dialog**. This provides a stronger security boundary and a cleaner UX.

---

## Other 2026 Agent Features for Integration

### 1. Claude "Agent Teams" (teammate-mode)
- **Feature:** Multiple agents collaborating on a shared task list (`Ctrl+T`).
- **`aterm` Integration:** A **"Team Dashboard"** in the sidebar. It would visualize the "Mailbox" (XML messaging protocol) between agents, showing who is working on what in real-time.

### 2. Codex "Permanent Life" (Kairos Daemon)
- **Feature:** A background service that maintains session persistence for long-running refactors.
- **`aterm` Integration:** `aterm` can act as the host for the **Kairos daemon**, providing a "headless session" type in the sidebar that users can monitor or reattach to visually.

### 3. Gemini "Context Caching" Visualizer
- **Feature:** Real-time feedback on project context loading.
- **`aterm` Integration:** A **"Context Progress Bar"** in the terminal chrome (similar to a download bar), indicating the readiness of the LLM's project knowledge.

---

## Conclusion

The `/buddy` feature represents a shift toward **"Emotional Terminal UI" (ETUI)**. `aterm` is uniquely positioned to lead this by providing a native, performant, and multi-session aware environment for these AI companions and collaboration tools.
