# aterm × Aside — agent-native surface benchmark (2026-08-16)

Research-only. Nothing here is implemented; every proposal is a proposal.

## What this document is

The [existing benchmark series](../polling-vs-event-benchmark.md) benchmarks aterm
against other **terminals** — Ghostty, Alacritty, Kitty, WezTerm, cmux — on rendering
and event-loop mechanics. This one benchmarks aterm against a **browser**, and not on
anything a browser and a terminal share. Aside is a Chromium AI browser (YC F25, Korean
founding team); the subject here is the *shape of the surface it exposes to a coding
agent*, which is a design problem aterm/telepty has independently and answers differently.

Six patterns, each measured against what aterm + telepty ship today.

### Source (Rule 38)

YouTube <https://youtu.be/OXg-0BXf_Yk> — "AI 브라우저 다 써봤는데 결국 여기로 갈아탔습니다 |
Aside Browser", 개발동생, 2026-08-15, 14:27. **Transcript re-fetched for this document**
(`~/.claude/skills/youtube-analyzer/scripts/analyze_youtube.py`, Korean auto-sub,
24,906 bytes); every Aside claim below is from that transcript or the video description,
not from Aside's own docs, which were not read.

The author states his own token comparison was **not a controlled benchmark** (5 pages,
one read each, no repetition). It is quoted here as his number, with that caveat attached
every time it appears.

### Method for the aterm/telepty side (Rule 39 — everything re-measured)

Measured on this machine, 2026-08-16, against telepty 0.8.1 (`~/projects/aigentry-telepty`,
`package.json:3`) and the aterm working tree. Two measurement scripts, both read-only:

- **Tool-definition size** — spawned `mcp-server/index.mjs` over stdio, sent
  `initialize` + `tools/list`, measured `JSON.stringify(result.tools).length`. `tools/list`
  never touches the daemon. Char counts elsewhere are `wc -c`.
- **`read-screen` cost** — `telepty read-screen <sid> --lines 24` against the four live
  non-`orchestrator` sessions, 6 consecutive samples each. `GET /api/sessions/:id/screen`
  is a pure read (`daemon.js:5105`). The production daemon and the `orchestrator` session
  were not written to, restarted, or injected into; the aterm GUI was not launched.

---

## 1. Pattern table

| # | Aside pattern (from the talk) | aterm/telepty today | Gap | Proposal | Effort |
|---|---|---|---|---|---|
| 1 | Two tiers: `aside exec` (agent with the user's full browser context) vs `aside repl` (JS REPL — deterministic scripted verify / screenshot / download) | Both tiers exist: `inject` (`cli.js:2833`) and `read-screen` (`cli.js:2805`). **Split is not named anywhere**; `read-screen` is filed under monitoring (`skills/telepty/SKILL.md:61`). Worse, tier 2 is not deterministic — see §3.1 | The deterministic tier reads a **byte ring with ANSI stripped** (`daemon.js:5118-5122`), not a rendered grid. Against a working TUI it returns 12–50 chars of repaint delta, not a screen | Expose the rendered-grid read that already exists for cmux (`daemon.js:2180-2182`) as `read-screen --rendered`, and name the two tiers in one doc | **M** |
| 2 | ONE skill, tool definitions <5k chars, vs Playwright MCP's 24 tools / ~18k chars. Author measured 2.3× avg (1.2–4.2×) fewer context chars over 5 sites — *not a controlled benchmark* | **3 MCP tools, 1,272 chars total** (measured, §2). Skill corpus 25,666 chars across 9 files, of which 1,617 chars is the always-on name+description layer | telepty is already well inside Aside's budget on the always-on axis. The gap is the opposite one: the shipped skill corpus **is not installed** in this ecosystem's own worker sessions (§2.3), so agents learn the CLI from role prose instead | Nothing to fix on size. Decide whether the 9 skills are a product or dead weight | **S** (decision only) |
| 3 | Persistent **login session** as the killer feature vs Playwright's fresh profile | Structural, not a feature: `telepty allow` wraps a CLI that is *already* authenticated; the agent's whole state is the live process. Sessions now survive a daemon restart (`src/session-store/persistence.js`) | None on the capability. `BOUNDARY.md:18` still declares the opposite ("sessions are in-memory; a daemon restart loses all sessions") — the doc is stale, flagged in its own `KNOWN DIVERGENCE` (`BOUNDARY.md:409`) | Fix the declaration when D1 resolves; state persistence as aterm's structural differentiator in the README | **S** |
| 4 | Permission modes: read-only / **guard** (approved folders, ask otherwise) / full-access; guard recommended as default | Peer-lane hard block (#533, `daemon.js:1438`, enforced `daemon.js:4668`), Origin guard (#806, `BOUNDARY.md:95`), token-for-every-caller (#820, `BOUNDARY.md:100`) | **No middle mode.** Worker sessions launch `--dangerously-skip-permissions` by default (`bin/session-start.sh:126`, `bin/orchestrator-boot.sh:209`, and it is aterm's default CLI arg string). Aside's *recommended* mode is the one we don't have; the mode we ship by default is Aside's most dangerous one | See §3.4 — a guard tier is wanted, and the cheapest honest version is per-session, not per-tool | **L** |
| 5 | **Profile** isolation (personal / work / full-access guest) + a rules-file default profile for autonomous runs | role-sandbox (#431, `bin/boot-prepare.mjs:18-27`, path at `:493-497`): per-session cwd with no project `CLAUDE.md`, plus a shadow config home for codex/gemini | Isolates **instructions**, not **credentials** — and deliberately: the shadow home "PRESERVING auth (auth.json / oauth_creds.json)" (`bin/boot-prepare.mjs:52-54`). Not user-selectable at runtime; the role is chosen by `dispatch.sh --role` | A credential-isolated role (Aside's "guest profile") is a real gap, but it is an OS/keychain problem, not a telepty one | **L** |
| 6 | Memory + **routines** (scheduled agent tasks) | aterm: none. Ecosystem: `aigentry-brain` MCP is the memory analog; the reconciler is the scheduling analog (`com.aigentry.reconciler.plist`, `bin/session-reconciler.sh:654-661`, 60s tick) | The reconciler is a **janitor**, not a routine: it reconciles state the operator did not ask about. There is no "run this task at 09:00" primitive anywhere | Record only. Not worth building at aterm's stage | **—** |

---

## 2. The measured number

Pattern 2 is the only pattern with a hard number on both sides, so it gets the detail.

### 2.1 What Aside claims

| Surface | Tools | Tool-definition chars |
|---|---|---|
| Playwright MCP | 24 | ~18,000 |
| Aside skill | 1 skill | <5,000 |

Result claimed: 2.3× average fewer context chars reading GitHub / Hacker News / MDN /
Wikipedia / Tailwind, range 1.2–4.2×. **The author explicitly says this is not a
controlled benchmark.** Treat the ratio as an anecdote and the two char counts as the
substantive claim.

### 2.2 What telepty actually costs — measured

**MCP tool definitions** (`mcp-server/index.mjs`, via a live `tools/list`):

| Tool | Total chars | description | inputSchema |
|---|---:|---:|---:|
| `telepty_list_sessions` | 265 | 76 | 85 |
| `telepty_session_status` | 361 | 50 | 206 |
| `telepty_inject_session` | 642 | 94 | 443 |
| **Total (3 tools)** | **1,272** | 285 | 734 |

**Skill corpus** (`skills/*/SKILL.md`, 9 files):

| Layer | Chars | What it measures |
|---|---:|---|
| name + `description` frontmatter, all 9 | **1,617** | the always-on cost where skills are installed with progressive disclosure |
| full bodies, all 9 | **25,666** | the ceiling if every skill body is pulled into one context |
| largest single body (`telepty-inject`) | 6,048 | the realistic cost of invoking the one skill an agent actually needs |
| `~/.claude/commands/telepty*.md`, 8 files | 7,829 | a **second, duplicate** surface teaching the same CLI |

**aterm's own agent-facing prose** (auto-discovered from the repo root, not a skill):
`aterm-context.md` 3,114 + `AGENTS.md` 9,828 = **12,942 chars**.

### 2.3 The headline comparison

| Surface | Always-on tool/skill definition chars | vs Playwright MCP (~18k) |
|---|---:|---|
| Playwright MCP | ~18,000 | 1.0× |
| Aside skill | <5,000 | ~3.6× smaller |
| **telepty MCP tools + skill descriptions** | **2,889** | **6.2× smaller** |
| **telepty as actually loaded in this session** | **~150** (8 bare command names) | ~120× smaller |
| telepty, every skill body loaded | 25,666 | 1.4× **larger** |

Four true numbers measuring four different things. Say which one you mean:

- **2,889** is the fair like-for-like against Aside's <5,000 — telepty already wins on the
  axis Aside is selling, and did so without ever framing it as a goal.
- **~150** is what a worker session in this ecosystem actually carries, because
  telepty's MCP server is **not wired into worker sessions at all** (the MCP servers
  loaded here are `aigentry-brain`, `claude-in-chrome`, `deliberation`, `snyk` — not
  telepty), and its 9 skills are **not installed** under `~/.claude/skills/` (only
  `telepty-deliberate`, from devkit, is). Agents learn `telepty inject` from role
  instruction prose. The 25,666-char corpus and the `skill-installer.js` that deploys it
  are shipped and unused here.
- **25,666** is the number to quote if anyone proposes installing all nine.

Worth stating plainly: this harness already does the thing Aside markets — MCP schemas
here are **deferred** and fetched on demand (93 tool names visible, zero schemas loaded
until requested). Progressive disclosure of tool definitions is a harness feature now,
not a differentiator of any one tool.

### 2.4 The per-observation cost — and the finding that matters more

The analog of "chars to read one page" is "chars to read one session screen".
`telepty read-screen <sid> --lines 24`, 6 consecutive samples per session:

| Session | Activity observation | Samples (chars) | min–max |
|---|---|---|---|
| `sp902-sp902-sweep` | `pty_quiet` (idle 29s) | 673, 673, 673, 673, 673, 673 | **673, stable** |
| `ts899-ts899-dispatch` | `output_observed` | 26, 24, 14, 24, 12, 34 | 12–34 |
| `rs912-rs912-memory` | `output_observed` | 34, 27, 26, 31, 29, 26 | 26–34 |
| `bm913-bm913-aside` | `output_observed` | 15, 15, 22, 50, 49, 49 | 15–50 |

An earlier 12-sample run on one actively-rendering session gave 9–570 chars, median 36.

**673 chars (~170 tokens) for a quiet 24-line screen** is the number to cite; the ceiling
for a fully dense 80×24 grid is 1,944 chars. Both are cheap — cheaper than any browser
page read, which is the expected result and not interesting.

The interesting result is the variance. `read-screen` returns a stable, complete screen
**only when the session is quiet**. While the session is working — precisely when an agent
polls it — it returns 12–50 chars. That is not a degraded screen, it is a different
object: the tail of a raw PTY byte ring with ANSI stripped, so a TUI repaint that moves
the cursor and rewrites a few cells yields a few chars of "screen".

This is a correctness finding, not a cost finding, and it is §3.1.

---

## 3. Per-pattern findings

### 3.1 Pattern 1 — two tiers, one of them not deterministic

Aside's two tiers are a deliberate split by *determinism*: `exec` is the LLM driving a
browser with your whole context; `repl` is scripted JavaScript, for when you want the same
answer twice. telepty has the same two verbs and never says so:

- **Tier 1, LLM-driven** — `telepty inject` (`cli.js:2833`) writes bytes to another
  agent's PTY. Non-deterministic by construction, and correct that way.
- **Tier 2, meant to be deterministic** — `telepty read-screen` (`cli.js:2805` →
  `daemon.js:5105`).

Tier 2 does not deliver determinism. `GET /api/sessions/:id/screen` joins the session's
`outputRing`, strips ANSI (`stripAnsiForScreen`, #715), splits on `\n` and returns the last
N lines (`daemon.js:5118-5127`). For a line-oriented shell that is a screen. For a
full-screen TUI that repaints by cursor addressing — every AI CLI we wrap — the escape
sequences *are* the geometry, and stripping them leaves the delta.

The rendered-grid read exists. It is cmux-only and internal: `submitGate.awaitPromptSymbol`
polls `cmux read-screen` for a prompt glyph (`daemon.js:2180-2182`, again at
`daemon.js:4404-4408`), and the code says so directly — "#29: non-cmux (warp/pty/kitty) has
**NO** rendered-screen read primitive" (`daemon.js:2204`). So telepty already depends on a
real screen model for its own gating and does not offer it to callers.

**aterm is the natural home for the fix.** aterm owns a terminal emulator: it has the grid.
A rendered-grid read exposed from aterm's IPC, surfaced as `read-screen --rendered`, gives
tier 2 the property it is supposed to have — and gives aterm a capability cmux currently
has and telepty's public surface does not.

Proposal: **M**. One IPC verb, one CLI flag, one paragraph naming the two tiers. The naming
is worth as much as the code — an agent choosing between "inject and hope" and "read the
screen" today has nothing telling it these are tiers of the same thing.

### 3.2 Pattern 3 — persistence is structural, and the doc denies it

Aside's differentiator is that its agent inherits a logged-in browser. Playwright starts
from a fresh profile, so anything behind a login is out of reach.

aterm/telepty gets the equivalent for free and one level deeper: `telepty allow` does not
*create* an authenticated context, it **wraps a process that already has one**. The agent's
credentials, its conversation, its loaded context, its cwd — all of it is the live process,
not a snapshot telepty restores. There is no fresh-profile mode to escape from.

What it enables is already the ecosystem's whole operating model: long-lived worker
sessions, the dispatch→report loop, an orchestrator injecting into a session that has been
alive for hours.

The one defect is documentation. `BOUNDARY.md:18` still declares "sessions are in-memory; a
daemon restart loses all sessions", which the same file's `KNOWN DIVERGENCE` table
(`BOUNDARY.md:409`) contradicts with evidence (`src/session-store/persistence.js`, restore
loop + 9 persist call sites). The declaration is frozen pending constitutional decision D1,
which is the right call for a boundary doc and the wrong state for a README. aterm's README
should say the true thing now: **persistent authenticated sessions, by construction.**

Proposal: **S**, README only.

### 3.3 Pattern 6 — record only

aterm has no memory and no routines. The ecosystem has memory (`aigentry-brain`, an MCP
server with 30 tools, running under `com.aigentry.brain-sync.plist`) but aterm does not
own or expose it.

The closest analog to a routine is the reconciler: `com.aigentry.reconciler.plist` runs
`bin/session-reconciler.sh --loop`, which re-execs a fresh `--once` tick every
`RECONCILER_LOOP_INTERVAL` (default 60s, `session-reconciler.sh:654-661`), with
`dispatch-cleanup-scheduler.sh tick` as its first responsibility. That is a **janitor** —
it reconciles state nobody asked about, prunes orphaned workspaces, fires deferred
cleanups. Aside's routines are the opposite thing: a user authors a task and names a time.

No proposal. Recorded so the next person does not mistake the reconciler for scheduling.

### 3.4 Pattern 4 — the mode we ship by default is the one Aside warns about

Aside offers three modes and **recommends guard**: read-only, guard (act inside approved
folders, ask otherwise), full-access.

telepty's guards are real, and they are all *lane* guards rather than *capability* guards:

| Guard | What it stops | Where |
|---|---|---|
| #533 peer-lane hard block | peer→peer injects that are not a sanctioned `ask-request`/`ask-reply` envelope — i.e. one worker tasking another behind the orchestrator's back | `daemon.js:1438` (policy), `daemon.js:4668` (enforcement, 403 `PEER_INJECT_BLOCKED`) |
| #806 Origin guard | a web page driving the local daemon; absolute — a valid token cannot buy past it | `BOUNDARY.md:95` |
| #820 credential-for-every-caller | reachability as authentication, including on loopback | `BOUNDARY.md:100` |

Two honest limits on that column. #533 **fails open** when no orchestrator sid is
configured (`daemon.js:1444-1446`) — deliberate, since fail-closed would brick the mesh,
but it means the guard is off in exactly the configuration nobody checked. And #820's own
doc states the boundary it buys is roughly the uid boundary: it **does not stop a same-uid
process** (`BOUNDARY.md:108`).

The gap is the middle mode. Worker sessions launch with `--dangerously-skip-permissions`
(`bin/session-start.sh:126`, `bin/orchestrator-boot.sh:209`; it is also aterm's default
CLI argument string for claude). In Aside's vocabulary this ecosystem runs **full-access by
default, on every session, with no read-only tier and no guard tier**.

Is a guard mode wanted? Yes — but not the per-tool-call approval Aside implies, which is
unworkable for a headless dispatched worker: there is nobody at that screen to approve
anything, which is why the flag is there in the first place. The two things that already
exist and are shaped like a guard are the **ambiguity gate** (`bin/hitl.sh` opened by the
orchestrator on a HOLD inject) and the **role contract** — both of which gate *tasks*, not
*capabilities*.

The cheapest honest guard tier is therefore per-session and declarative, not per-call:
a role that runs without `--dangerously-skip-permissions` and with a read-only tool
allowlist, used for analyst/architect dispatches that have no business writing anything.
This document's own dispatch says "READ-ONLY except your ONE output" — enforced today by
nothing but this session's willingness to comply.

Proposal: **L**, and it is a policy decision before it is code. Scoped down to
"architect/analyst roles spawn without the bypass flag", it is **M**.

### 3.5 Pattern 5 — profiles isolate instructions, not credentials

Aside's profiles (personal / work / full-access guest) isolate **login sessions, cookies
and history**, and the author's practice is to point autonomous runs at the guest profile
via a rules-file default — a blank profile so a compromised agent has nothing to steal.

role-sandbox (#431) isolates a different axis. `bin/boot-prepare.mjs` gives each session a
cwd at `$HOME/.aigentry/role-sandbox/<role>-<sid>/` so the wrapped CLI's upward `CLAUDE.md`
walk finds nothing (`:18-27`, `:493-497`), attaches the role contract via
`--append-system-prompt-file`, and for codex/gemini redirects `CODEX_HOME`/`GEMINI_CLI_HOME`
to a shadow home that mirrors the real one minus the global context doc. That is **context
isolation**, and it is well built — it exists because a project `CLAUDE.md` contaminated a
session's role in a real 2026-05-23 incident.

What it does not isolate is credentials, and that is explicit: the shadow home
"PRESERVING auth (auth.json / oauth_creds.json)" (`:52-54`). Every session runs as the same
uid with the same `~/.claude` credentials, the same git credentials, the same keychain.
There is no guest profile. The session you are reading has full-access CLI permissions and
the operator's credentials, sandboxed only by a cwd and a system prompt.

Two answers to the dispatch's question:

- **Is it user-visible/selectable per session?** Selectable, not user-visible: the role is
  an argument to `dispatch.sh --role`, resolved at spawn. There is no runtime profile
  switch and no `profile list` equivalent — the closest is reading the boot contract in
  the session's own system prompt.
- **Is a credential-isolated profile wanted?** Yes, and it is **L**, because it is an
  OS/keychain problem rather than a telepty one. `BOUNDARY.md:120` already names the
  ceiling: *"Same-uid is not a boundary telepty can create; only the OS can."* A real guest
  profile means a separate uid or a sandbox profile with a different `HOME` — the same
  lever #820 was the precondition for.

---

## 4. Prompt-injection parity

The talk cites OpenAI's Atlas position — prompt injection is unsolved, and expected to
need years more work — and treats Aside's mitigations as partial by admission: permission
modes, a password manager that fills credentials without exposing plaintext to the agent
(documented, and the author says he did not verify it), profile isolation as the user's own
last line.

telepty's #806 drive-by guard is **not parity with that, and should not be described as
parity**. #806 defends the *control plane*: a request carrying a browser `Origin` must name
an allowlisted one, absolutely, ahead of any credential check (`BOUNDARY.md:95`), with
`Origin: null` refused rather than treated as absent and browser WebSocket handshakes
refused at upgrade (`test/loopback-drive-by-guard.test.js:109`, `:177`). It stops a web page
the operator happens to have open from driving the daemon and injecting into live sessions.
Prompt injection is a *content-plane* attack: an agent reads a poisoned page or file and
acts on text inside it, using its own legitimate credentials, through channels no origin
guard sees. #806 does nothing about that, by design, and neither does anything else in the
tree. The nearest content-plane mitigation is #533, which limits **lateral movement** — a
compromised worker cannot task its peers without a sanctioned envelope (`daemon.js:4668`) —
and that is containment after the fact, not prevention. The honest parity statement is:
telepty has a stronger control-plane guard than a browser needs and the same unsolved
content-plane problem as everyone else, minus Aside's permission modes.

---

## 5. Verdict — what is worth adopting

Two, and only two.

**1. Name the two tiers, and make the deterministic one deterministic (§3.1, M).** This is
the adopt candidate. Aside's insight is not that it has two entry points, it is that it
told agents which one is repeatable — and telepty ships both verbs, documents neither as a
tier, and its "deterministic" verb returns a torn byte-ring delta whenever the target is
actually working (12–50 chars vs 673 quiet, measured §2.4). aterm owns a real terminal grid
and can close this where cmux already has: expose the rendered read as
`read-screen --rendered`, and write the one paragraph that says `inject` is the LLM tier and
rendered `read-screen` is the scripted tier. Highest value per line of code in this
document, and it fixes a correctness bug that is currently invisible because nothing asserts
on `read-screen` output.

**2. A no-bypass role for read-only dispatches (§3.4, M scoped).** Aside recommends guard
mode as the default; this ecosystem's default is full-access on every session. The general
guard mode is L and mostly policy, but the narrow version is cheap and lands where it
matters: analyst/architect/research roles spawn without `--dangerously-skip-permissions`.
Those dispatches are already declared read-only in prose — this makes the declaration
enforced instead of trusted.

**Not worth adopting.** Pattern 2 (already 6.2× under Playwright's budget and ~120× under
it as actually loaded — nothing to fix, only a decision about nine uninstalled skills).
Pattern 3 (aterm already wins structurally; the work is a README line). Pattern 5's guest
profile (real gap, but an OS boundary telepty cannot create — `BOUNDARY.md:120`). Pattern 6
(routines are a product feature for a product with users; aterm is 0.2.x).

---

## 6. Open questions for the owner

1. **The nine skills.** `skills/*/SKILL.md` (25,666 chars) plus `skill-installer.js` ship in
   telepty and are installed in **none** of this ecosystem's worker sessions. Product for
   external users, or dead weight to delete? The measurement says nothing about which — it
   only says they are not load-bearing here.
2. **Guard tier ownership.** A no-bypass role is a change to `bin/session-start.sh` /
   `boot-prepare.mjs` in **aigentry-orchestrator**, not to aterm. Does this land as an
   orchestrator task, and does the architect role want to be the first one to lose its
   bypass flag?
3. **`read-screen --rendered` boundary.** The rendered grid lives in aterm (and in cmux).
   Does telepty gain an aterm-backed screen path, or does `aterm read-screen` become a
   first-class verb and telepty stay byte-ring-only? This is a BOUNDARY.md question and it
   touches the unresolved D1 divergence.
4. **`--dangerously-skip-permissions` as aterm's shipped default.** It is the default CLI
   argument string for claude in aterm's settings, i.e. it is what an external aterm user
   gets. Intended?

## 7. What was not measured

- **aterm's own CLI was never executed.** The GUI must not be launched, and `npm/aterm/bin/aterm.js:55-68`
  routes any invocation to activating the running app when one is already running, so
  `aterm list` could not be run without violating the constraint. The aterm CLI surface here
  is read from `README.md:27-34`, `aterm-context.md:27-65` and `CHANGELOG.md:113-115` only.
  Note in passing, unverified: `ATERM_IPC_SOCKET` is *set* in `aterm-core/src/pty.rs:985`
  and read nowhere in this repo — someone should confirm which binary serves the documented
  in-workspace CLI.
- **Aside itself.** Not installed, not run. Every Aside number is the video author's, and
  his token comparison is uncontrolled by his own statement.
- **Token counts.** Everything here is characters. Tokenization varies by model and the
  Korean/box-drawing content in a terminal screen tokenizes worse than English prose;
  ~170 tokens for 673 chars is an estimate, not a measurement.
- **`read-screen` on the `orchestrator` session.** Excluded by the dispatch constraint, so
  the sample is 4 sessions, not 5.
- **Whether the six patterns are the right six.** They are the ones the talk covers.
