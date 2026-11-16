# Leftovers and ports

Status: Draft
Size: Full — a new page, a new kind of remembered state (ended agent sessions), new CLI and MCP tools, and it stops processes.
Owner: Henrik · Started: 2026-11-16 · Approved: not yet
Working assumptions: none yet; the `Proposed` decisions in section 5 carry a recommendation each.

Tidegauge 1.0's headline. **Cleanup** removes what agents leave on disk; this
stops what they leave **running**. You see every listening port with who
started it, you get a warning when a dev server is open to the network, and
when a Claude Code or Codex session ends, Tidegauge lists the servers it left
behind with one Stop for all.

## 1. Requirements

### Sources

| ID | Location | What it contributes |
|---|---|---|
| S-1 | Issue #12, "Leftovers and ports (headline)", 2026-11-16 | The four items: Ports page, agent leftovers, exposed-port warning, MCP and CLI |
| S-2 | Brief from the planning session, 2026-11-16 | Pairs with Cleanup; Stat Space's port tracing and "agent finished, servers didn't" as the bar; files to avoid (#127–#129) |
| S-3 | CONTRIBUTING.md | Bilingual UI through `locale::text`; tests in both languages; rendered check |
| S-4 | Existing code | `projects.rs` (ports via libproc, guarded `stop_targets`), `agents.rs` (session trees), `process_control.rs` (`blocked`, `signal`), `cleanup/cli.rs` (MCP), `cleanup/hooks.rs` (SessionEnd hook) |

### Requirements

| ID | Requirement | Priority | Acceptance criterion | Sources |
|---|---|---|---|---|
| FR-1 | List every listening TCP port of the current user's processes: port, bind address, process (name, PID, uptime), project folder when there is one. | Must | Given `python3 -m http.server 8765` in a git folder, the Ports page and `tidegauge ports` show 8765, `python3`, that folder, and bind `0.0.0.0`. | S-1 |
| FR-2 | Name each listener's launcher: an agent (Claude Code, Codex, other known agents), a terminal app, an editor, Docker, a launchd job (with its label), an app (itself), or "ended agent session". | Must | Unit tests over fake process trees give the right launcher for each kind; a server started from Terminal reads "Terminal", one started by a Claude Code session reads "Claude Code". | S-1 |
| FR-3 | Per listener: **Stop**, **Open in browser** (`http://localhost:<port>`), **Reveal folder** (Finder). | Must | Stop ends a test server and the row disappears; Open launches the default browser; Reveal selects the folder. Stop is absent for blocked processes (FR-11). | S-1 |
| FR-4 | When an agent session ends, list what its process tree left running: servers (with ports) and other processes, grouped under the session (agent, folder, ended time). | Must | A fake ended session whose child still runs appears under "Left running" with that child; a child that exits drops off; a session with nothing left is not shown. | S-1, S-2 |
| FR-5 | One **Stop all** per ended session, with a confirmation that lists what will stop. | Must | Stop all on a test session ends all its remembered, still-matching processes and reports each outcome. | S-1 |
| FR-6 | Remember ended sessions' leftovers across a Tidegauge restart, for 24 hours or until nothing is left. | Should | Restarting the app keeps the "Left running" list; an entry older than 24 h is dropped. | S-2 |
| FR-7 | The SessionEnd hook records the session's tree at the moment it ends, so leftovers are known even when the app missed the end or was not running. | Should | `tidegauge hook session-end` run under a fake agent tree writes that tree to the ended-sessions file. | S-1, S-4 |
| FR-8 | Flag a dev server (has a project, or an agent launcher) bound to all addresses (`0.0.0.0`, `::`) or a LAN address, and say in plain words what that means and how to fix it. | Must | `http.server` on 0.0.0.0 shows the warning; the same on `--bind 127.0.0.1` does not. The text names "other devices on this network" and `127.0.0.1`. | S-1 |
| FR-9 | `tidegauge ports` prints the table; `--json` prints the same as JSON. | Must | Command output contains the test server's row; `--json` parses. | S-1 |
| FR-10 | `tidegauge mcp` gains `whats_on_port` (read-only) and `stop_port` (gated, FR-12). | Must | JSON-RPC tests: `whats_on_port {port}` returns the listener; `stop_port` refuses while the setting is off and stops a test server when it is on. | S-1 |
| FR-11 | Stop is never offered or performed for processes `process_control::blocked()` refuses (other users, session-critical, Tidegauge itself, unverifiable), and every stop re-checks PID start time and executable before signalling. | Must | Unit tests: blocked kinds give no Stop; a changed start time is refused. | S-4 |
| FR-12 | An agent may stop a port through MCP only when the user turned on "Let agents stop dev servers" (off by default), and only a dev server inside the calling agent's project. | Must | MCP tests for setting off, other project, and allowed case. | S-2 |
| FR-13 | Docker and launchd listeners show their kind instead of a bare Stop that would kill Docker or be undone by launchd. | Should | A `com.docker.backend` listener reads "Docker" with no Stop; a launchd job shows its label and "launchd restarts it". | S-1 |
| NFR-1 | No extra cost while the Ports page is closed: ports are read only on the existing heavy pass and listener cache (8 s / 30 s hidden). Session memory adds at most a few hundred small records. | Must | Idle CPU/memory unchanged within noise (compare with 0.9.0 on the same Mac). | S-2 |
| NFR-2 | Privacy: nothing leaves the Mac except what an agent asks for through MCP; command-line arguments and environment are never stored or returned (they may hold secrets). | Must | Code review; the ended-sessions file and MCP output contain no `args` field. | S-2 |
| NFR-3 | Every UI string bilingual (English, Norwegian Bokmål). | Must | `locale_guard` passes; `cargo test --locked` passes with and without `TIDEGAUGE_TEST_LANG=nb`. | S-3 |

### Traceability

| Source statement | Disposition |
|---|---|
| "every listening port with its process, project folder and launcher" | FR-1, FR-2 |
| "Stop, Open and Reveal folder" | FR-3, FR-11, FR-13 |
| "when a session ends, list the servers and processes it left running" | FR-4, FR-6, FR-7 |
| "one Stop for all" | FR-5 |
| "flag a dev server bound to 0.0.0.0 or a LAN address, with what that means" | FR-8 |
| "`whats_on_port` and `stop_port`" / "`tidegauge ports`" | FR-10, FR-12 / FR-9 |
| Stat Space traces root-owned and other users' ports | Later (L-1) |

## 2. Scope

- **In:** FR-1–FR-13, NFR-1–NFR-3.
- **Out:** UDP ports; remote machines; stopping Docker containers (Tidegauge shows them, Docker stops them); editing an agent's config (Tidegauge still only shows snippets).
- **Later:**
  - L-1: ports owned by root or other users (libproc can't read their sockets unprivileged; `sysctl net.inet.tcp.pcblist_n` could show them without a process). Q-4.
  - L-2: an opt-in alert "Claude Code ended and left 2 servers running". Q-3.
  - L-3: Docker container names via the `docker` CLI; `launchctl bootout` for launchd jobs.
  - L-4: the "responsible process" API to name a launcher after its parent died (private API).

## 3. Conceptual design

### Purpose

Answer "what is on port 3000, who started it, and can I stop it?" and "what
did my agent leave running?" in one place, without root.

### Context map

```
 agents (Claude Code, Codex) ──MCP stdio──▶ tidegauge mcp ──┐
 agent SessionEnd hook ───────────────────▶ tidegauge hook ─┤
 you (Terminal) ──────────────────────────▶ tidegauge ports ┤──▶ ports scan ──▶ libproc (kernel)
 you (dashboard) ───────────────────────▶ Ports / Agents UI ┘        │            launchctl list
                                                                     ▼
                                       ended-sessions.json (config dir, local)
```

Trust boundaries and threats:
- **Agent → MCP:** an agent is semi-trusted. Threat: an agent (or a prompt injection) stops servers the user needs. Answer: `stop_port` off by default, limited to the caller's own project, dev servers only (FR-12).
- **PID reuse:** threat: Stop hits an unrelated process that got the same PID. Answer: every stop re-reads start time and executable just before the signal (existing `stop_targets` and `process_control::signal` already do).
- **Data at rest:** the ended-sessions file holds folder paths, process names and ports, no arguments (NFR-2). It lives with Tidegauge's other settings.

### Capabilities

| Capability | FR IDs |
|---|---|
| See listeners with project, launcher and bind | FR-1, FR-2, FR-13 |
| Act on a listener | FR-3, FR-11 |
| Remember sessions and what they left | FR-4, FR-6, FR-7 |
| Stop what a session left | FR-5, FR-11 |
| Warn about exposure | FR-8 |
| Ask and act from a terminal or an agent | FR-9, FR-10, FR-12 |

### Components

New code goes in `src/ports/` so it stays clear of the files open PRs touch.

| Component | Responsibility | Requirement IDs |
|---|---|---|
| `ports::scan` | Read listeners with bind address; join process, project, launcher | FR-1, FR-8, NFR-1 |
| `ports::launcher` | Walk the parent chain and name the launcher | FR-2, FR-13 |
| `ports::ended` | Track live sessions' members; detect the end; keep and expire leftovers; the JSON file | FR-4, FR-6, FR-7, NFR-2 |
| `ports::stop` | One stop path for a listener or a leftover, built on `stop_targets` and `process_control::signal` | FR-3, FR-5, FR-11, FR-12 |
| `ports::cli` | `tidegauge ports`, the two MCP tools | FR-9, FR-10, FR-12 |
| Ports page (UI) | The table, actions, warning | FR-1–FR-3, FR-8, NFR-3 |
| Agents page "Left running" (UI) | Ended sessions and Stop all | FR-4, FR-5, NFR-3 |
| `ffi::socket_fdinfo` (existing, extended) | Bind address and family | FR-1, FR-8 |

### Domain concepts

- **Listener:** one process listening on one TCP port, on one or more bind addresses (IPv4 and IPv6 rows merge).
- **Launcher:** what started it, found by walking up the parent chain.
- **Exposed:** a listener on a wildcard or non-loopback address.
- **Member:** a process seen inside a live agent session's tree, remembered by PID and start time.
- **Leftover:** a member still running, with the same start time, after its session's root process is gone.

## 4. Logical design

### Component contracts

| Component | Provides | Guarantees | Depends on |
|---|---|---|---|
| `ffi::socket_fdinfo` | `listening_tcp(&self) -> Option<(u16, Bind)>` with `Bind::{Any, Loopback, Addr(IpAddr)}` | Offsets checked against `<sys/proc_info.h>` in a test | — |
| `ports::scan` | `listeners(procs, max_age) -> Vec<Listener>` | Same cache as `detect_projects`, so no extra libproc pass | ffi, `projects::project_root`, launcher |
| `ports::launcher` | `launcher(pid, by_pid, sessions, ended, launchd) -> Launcher` | Pure over its inputs; testable with fake trees | `agents::classify`, `bundle_root` |
| `ports::ended` | `observe(sessions, procs, now)`, `leftovers() -> Vec<EndedSession>`, `record_from_hook(tree)` | Never lists a process whose start time differs from the one remembered | config dir, flock like `hooks::with_file` |
| `ports::stop` | `stop_listener(&Listener, Force)`, `stop_all(&EndedSession, Force)` | Refuses `blocked()`; re-verifies identity; reports per process | `projects::stop_targets`, `process_control::signal` |
| `ports::cli` | `ports [--json]`, MCP `whats_on_port`, `stop_port` | JSON with no args/env | scan, stop, settings |

### Domain model

```rust
struct Listener { pid, port: u16, binds: Vec<Bind>, name, exe_path, start_unix,
                  project: Option<String>, launcher: Launcher, exposed: bool }
enum Launcher { Agent(Agent), EndedSession(Agent), Terminal(String), Editor(String),
                Docker, Launchd(String /*label*/), App(String), Unknown }
struct Member { pid: i32, start_unix: f64, name: String, ports: Vec<u16> }
struct EndedSession { agent: Agent, folder: Option<String>, root_pid: i32,
                      ended_at: f64, members: Vec<Member> }
```

### Data ownership

| Datum | Owner | Readers |
|---|---|---|
| Listeners (live) | `ports::scan`, in the engine snapshot | Ports page, CLI, MCP |
| Live session members | `ports::ended` (in memory) | itself |
| Ended sessions | `ports::ended` → `ended-sessions.json` in the config dir | Agents page, Ports page (launcher), CLI |
| launchd labels | `ports::launcher` (cached `launchctl list`, 60 s) | scan |
| "Let agents stop dev servers" | `settings` | `ports::cli` |

### How a session's end is detected and remembered (D-2)

1. Every heavy pass (2 s watched, 8 s hidden) `agents::sessions` already returns each live session's tree. `ports::ended::observe` adds every tree member to that session's **sticky member set**, keyed by root (PID, start time). A child that daemonizes later (reparents to launchd) stays a member, because membership is never removed while the session lives.
2. A session has **ended** when its root (PID, start time) is no longer in the process list.
3. Its leftovers are the members still alive with the same start time, minus shells, minus members now inside another live session. If none, it is forgotten.
4. The ended session is written to `ended-sessions.json` (FR-6). Each pass drops members that exited; a session with no members, or older than 24 h, is removed.
5. **Hook path (FR-7):** `tidegauge hook session-end` runs while the agent is still alive. It walks up from its own parent to the agent root (`agents::classify`), collects that root's descendants, and merges them into the same file under the same lock. This catches the end even when the app was not running, and children that lived and detached between two passes.

Known gap: a child that starts and detaches inside one pass interval, with no hook installed, is never seen. Its launcher then reads "launchd" or "Unknown". L-4 could close this later.

### Scenario walkthroughs

**A. "What's on 3000?" from the dashboard.** (1) Engine heavy pass → `scan::listeners` reads the cached sockets (FR-1). (2) `launcher` walks PPIDs: `node` → `zsh` → `claude` → Claude Code (FR-2). (3) Page shows 3000, `node`, `~/Dev/Repos/app`, Claude Code, bind `*` with the exposed badge (FR-8). (4) Stop → confirm → `stop::stop_listener` → `stop_targets` re-verifies and sends SIGTERM (FR-3, FR-11).

**B. Agent finished, server didn't.** (1) Session root `claude` (PID 900) is seen with child `vite` (PID 950) for an hour; `observe` remembers 950 (FR-4). (2) The user closes the session; 900 exits, 950 is reparented to launchd. (3) Next pass: 900 gone → ended; 950 alive with the same start → leftover; written to file (FR-6). (4) Agents page shows "Claude Code · app · ended 2 min ago — Left running: vite :5173 · Stop all" (FR-4). (5) Stop all → confirm list → `stop::stop_all` → each re-verified, signalled, reported (FR-5, FR-11). Ports page shows 5173's launcher as "Claude Code (ended)" until then.

**C. Agent asks over MCP.** (1) Agent calls `whats_on_port {port: 5173}` → JSON with process, project, launcher, exposed (FR-10). (2) Agent calls `stop_port {port: 5173}` → setting off → refusal text naming the setting (FR-12). (3) With the setting on, and the server's project equal to the MCP process's working-directory project → stop through `stop_targets` (FR-11).

### NFR mechanisms

| NFR | Mechanism | Arithmetic |
|---|---|---|
| NFR-1 | Reuse the `LISTENERS` cache; bind address read from the same 792-byte struct already fetched; launcher walk over the in-memory `by_pid` map; `launchctl list` at most once a minute and only when a listener has PPID 1 | ~30 listeners × ≤32 parent hops = <1,000 map lookups per pass; member sets ≈ sessions × ≤256 (existing `MAX_TREE`) × ~40 bytes ≈ 100 KB worst case |
| NFR-2 | `Member` has no args field; MCP JSON built from `Listener` only | — |
| NFR-3 | `locale::text` everywhere; `locale_guard` | — |

### Risk register

| Risk | Likelihood | Effect | Answer |
|---|---|---|---|
| Socket struct offsets wrong for the bind address | Low | Wrong exposure warnings | Offsets from the SDK header in a test, plus a live check with a server on 0.0.0.0 and 127.0.0.1 |
| A "leftover" is something the user wants kept (a long-running server started by the agent on purpose) | Medium | Annoyance, not harm | Nothing stops without a confirmation; an entry can be dismissed |
| False "ended": the agent restarts its root process under a new PID | Low | A spurious entry | Shown only when members survive; dismissable |
| `launchctl list` output changes | Low | Launchd label missing | Falls back to "launchd" without a label |
| Merge conflicts with #127 (agents.rs, agents_panel.rs) | High | Rebase work | New code in `src/ports/`; the Agents page change is one call site, built last |

## 5. Decisions

### D-1: The Projects tab becomes the Ports page (Proposed, 2026-11-16)
Context: the dashboard already has a Projects tab listing dev servers by project (`ui::projects_tab`).
Options: (a) rename Projects to **Ports** and widen it to every listener, grouped by project with an "Other" group; (b) add a separate Ports page next to Projects.
Choice (recommended): (a). One place to look; dev servers stay grouped by project at the top; no near-duplicate pages.
Consequences: the tab label, badge count and help text change; Settings that refer to "Projects" follow.

### D-2: Session ends are detected by polling, with the SessionEnd hook as a second source (Proposed, 2026-11-16)
Context: an ended agent's children reparent to launchd, so the tree is lost unless remembered.
Options: (a) poll only; (b) hook only; (c) both, merged in one file.
Choice (recommended): (c). Polling works with no setup (Codex has no hook here); the hook is exact when installed.
Consequences: one file with a lock shared by app and hook; FR-7 is a separate slice.

### D-3: What Stop does per launcher (Proposed, 2026-11-16)
Context: one SIGTERM is right for a dev server, wrong for Docker (kills all containers) and useless for a launchd KeepAlive job.
Choice (recommended): dev servers in a project → existing `stop_targets`; other own processes → `process_control::signal` with the "part of macOS" warning; Docker → no Stop ("Stop it in Docker"); launchd → no Stop, show the label (L-3 later); blocked → no Stop.
Consequences: FR-13; Stop all for leftovers uses `process_control::signal`, because a leftover need not be a dev server.

### D-4: `stop_port` is limited to the caller's project (Proposed, 2026-11-16)
Context: an agent should clean up its own servers, not someone else's.
Options: (a) any dev server once the setting is on; (b) only dev servers whose project equals the project of the MCP server's working directory (the agent's folder, git top-level, worktrees included).
Choice (recommended): (b). Matches "your agent's servers"; refusal text says why.
Consequences: an agent in another folder can see but not stop.

## 6. Coverage

| Requirement | Component | Slice |
|---|---|---|
| FR-1, FR-2, FR-8, FR-13 | scan, launcher, ffi | 1 |
| FR-9 | cli | 1 |
| FR-10, FR-12 | cli, stop | 2 |
| FR-3, FR-11 | stop, Ports page | 3 |
| FR-4, FR-5, FR-6 | ended, stop, Agents page | 4 |
| FR-7 | ended, hooks | 5 |
| NFR-1–3 | all | every slice |

## 7. Build plan

| Slice | What it delivers | Requirement IDs | Status |
|---|---|---|---|
| 1 | Bind address in ffi; `ports::scan` + `launcher`; `tidegauge ports [--json]`. Thinnest end-to-end path through the riskiest contract (socket layout and launcher walk). | FR-1, FR-2, FR-8, FR-9, FR-13 | Planned |
| 2 | MCP `whats_on_port`, `stop_port` and the setting | FR-10, FR-11, FR-12 | Planned |
| 3 | Ports page (D-1) with Stop, Open, Reveal and the exposed warning | FR-1, FR-3, FR-8, FR-11 | Planned |
| 4 | `ports::ended` + "Left running" on the Agents page with Stop all; launcher "ended session" | FR-4, FR-5, FR-6 | Planned (after #127 merges) |
| 5 | SessionEnd hook records the tree | FR-7 | Planned |

Each slice is its own PR with a CHANGELOG line under `## Unreleased`.
Slices 1–2 can ship as one PR, 3 as one, 4–5 as one.

## 8. Open questions

| ID | Question | Blocks | Asked of |
|---|---|---|---|
| Q-1 | D-1: turn Projects into Ports (recommended), or a new page? | Slice 3 | Henrik |
| Q-2 | D-4: `stop_port` only for the caller's own project (recommended), or any dev server? | Slice 2 | Henrik |
| Q-3 | L-2: an opt-in alert when a session leaves servers running — now or later? | Nothing (Later) | Henrik |
| Q-4 | L-1: show a count of ports owned by macOS or other users that Tidegauge can't attribute? | Nothing (Later) | Henrik |

## 11. Codebase deltas and change log

### Codebase deltas

| Where code and design differ | Ruling |
|---|---|
| `socket_fdinfo::listening_tcp_port` drops the bind address | Code migrates: returns the bind too (slice 1) |
| `detect_projects` keeps only project listeners | Code migrates: `ports::scan` reads the same cache for all listeners; `detect_projects` stays for the Agents page |

### Change log
- 2026-11-16: first draft.
