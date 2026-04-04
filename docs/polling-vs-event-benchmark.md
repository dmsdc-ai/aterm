# aterm Benchmark Report: Polling vs. Event-Driven Architecture

This document evaluates aterm's current polling-heavy architecture against modern industry leaders (Ghostty, Alacritty, Kitty, WezTerm) and provides a roadmap for the event-driven transition (Phase 1).

## 1. Comparative Analysis Table

| Pattern | Ghostty | Alacritty | Kitty | WezTerm | aterm (Current) | aterm (Target) |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Inject/IPC** | **SPSC Mailbox + libxev**. Producers notify IO thread. | **`mpsc` + `poller.notify()`**. Thread-safe UDS. | **Unix Socket + talk_loop**. Dedicated listener thread. | **Async UDS + Domain Mux**. Typed streams. | 500ms poll loop | **Condvar / Channel** |
| **Shell Ready**| **Immediate**. No artificial wait or detection. | **Immediate**. Processes stream from spawn. | **OSC 133 sequences**. Shell integration reports prompt. | **Domain Events**. Lifecycle notifications. | 2s fixed delay | **PTY Prompt Event** |
| **UI Refresh** | **`xev.Async` + 8ms cap**. Leading-edge coalescing. | **`Event::Wakeup` Proxy**. Leading-edge redraw. | **`mark_os_window_dirty`**. Event-driven push. | **`MuxNotification`**. Global Pub/Sub. | 1s periodic timer | **IPC Event Push** |
| **Process Exit**| **`xev.Process`**. Kernel-level (kqueue/pidfd). | **Signal Pipe**. SIGCHLD to poller event. | **Signal Pipe + waitpid**. REAP thread monitor. | **PTY EOF + Domain Mux**. Stream closure tracking. | 1s periodic timer | **SIGCHLD Signal Pipe** |
| **Sub-process** | **Dual detection**. PTY close + `xev.Process`. | **SIGCHLD**. Global process handler. | **--wait-for-child**. Remote command queue. | **Multiplexed Domains**. Native sub-task tracking. | 10s Python poll loop | **wait-until Event** |

## 2. Performance Impact Analysis

| Logic Conversion | CPU Impact (Idle) | Latency Impact | Battery/System Impact |
| :--- | :--- | :--- | :--- |
| **Rust Injector** | 2 wakeups/sec → 0 | 500ms → <1ms | Reduced context switching. |
| **Swift Sidebar** | 1 IPC/sec/WS → 0 | 1000ms → ~100ms (coalesced) | Massive reduction in main-thread IPC overhead. |
| **Shell Ready** | N/A | 2000ms → ~200ms | Snappier UX; eliminates arbitrary "waiting" feel. |
| **Process Exit** | 1 poll/sec → 0 | 1000ms → <5ms | Instant cleanup of "dead" sessions. |
| **Python Dispatch**| Continuous 10s poll | 10s jitter → <10ms | Critical for high-volume task orchestration. |

## 3. Unique Patterns to Adopt

- **From Ghostty**: **Leading-edge Coalescing (25ms-50ms)**. Ensure UI updates don't overwhelm the main thread during high-frequency output while maintaining "instant" feel for the first event.
- **From Alacritty**: **Absolute Deadline Scheduler**. Use `Instant` based deadlines for the injector to prevent starvation when notifications are frequent.
- **From WezTerm**: **Multiplexed IPC Channel**. Use a single UDS channel for all workspace events to prevent File Descriptor exhaustion.
- **From Kitty**: **Signal Pipe Pattern**. Reliably convert UNIX signals to internal event loop wakeups to avoid race conditions in process reaping.

## 4. Risk Assessment

- **Low Risk**: **SIGCHLD Pipe**. Well-understood pattern; low chance of regressions.
- **Medium Risk**: **Condvar Injector**. Requires careful handling of the "force-inject" fallback to ensure messages aren't stuck if prompt detection fails.
- **High Risk**: **Prompt Detection (Heuristic)**. PTY output is noisy. False positives (catting a file with prompts) or negatives (complex TUI) remain the biggest stability hurdle.

## 5. Implementation Priority Ranking

1. **Rust Injector (HIGH)**: Immediate impact on battery and injection reliability.
2. **Python Dispatch (HIGH)**: Required for efficient AI-orchestration and sub-task scaling.
3. **Swift Sidebar (HIGH)**: Fixes UI "lag" and reduces main-thread pressure.
4. **Shell Ready (MEDIUM)**: Significant UX improvement ("feels fast").
5. **Process Exit (MEDIUM)**: System hygiene and reliability.
6. **Low Priority Items**: Devkit init, WS reconnect, Task queue (can remain as slow polling/backoff).

---
*Report compiled by aigentry-aterm-gemini*
