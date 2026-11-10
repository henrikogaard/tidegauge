# Cleanup page

Status: Building (slices 1–18 implemented; manual checks in section 10 open)
Size: Full — a new page, new git and agent integrations, and it removes user data.
Owner: Henrik · Started: 2026-11-09 · Approved: 2026-11-09, Henrik ("do all 6 slices in one go")
Working assumptions: the draft answers to Q-1–Q-9 in section 8 are adopted
(2026-11-09) so every slice can be designed now; a ruling from Henrik
replaces them. Every slice's detailed design is written up front (section 9)
so the slices can be built one after another without returning to planning.

Tidegauge gets a **Cleanup** page that finds what is safe to remove, with
stale agent worktrees first, and removes it only after you confirm. The look
follows Levo; the safety rules follow disk-jockey and Tidegauge's own
Trash-first rule.

## 1. Requirements

### Sources

| ID | Location | What it contributes |
|---|---|---|
| S1 | Henrik, chat 2026-11-09: "assess this page/app, and see if it has some cleanup use cases" | The goal: port cleanup ideas into Tidegauge |
| S2 | Henrik, chat 2026-11-09: "clean and pretty UI … help me clean up since there is a lot of stale worktrees and files after agent. I like the [Levo] app/ui" | Levo-style UI; agent worktrees and leftovers first |
| S3 | Henrik, chat 2026-11-09: "not against integrating with AI agents to do intelligent cleanup if the user wants it (must be guarded/confirmed)" | Optional AI review, opt-in and confirmed |
| S4 | Levo, https://omggg.wtf/levo/ (read 2026-11-09) | UI: free-space header, colour bar, levels, category rows, one Clean up button, lock chips, agent check |
| S5 | disk-jockey README, github.com/bmcreations/disk-jockey (MIT, read 2026-11-09) | Worktree states, orphan build data, tiers, protected paths, busy checks, preflight |
| S6 | Tidegauge code at 34a6bc2: `caches.rs`, `tools_panel.rs`, `ui.rs`, `alerts.rs`, `DESIGN.md`, `CONTRIBUTING.md` | Current behaviour and rules that must hold |
| S7 | Measurement on Henrik's Mac, 2026-11-09 | 86 folders in 11 `<repo>/.claude/worktrees` folders use about 135 GB (brev 38 GB, octant 29 GB, rista 26 GB, tidegauge 18 GB, auroradocs 14 GB) |

### Requirements

| ID | Requirement | Priority | Acceptance criterion | Sources |
|---|---|---|---|---|
| FR-1 | A Cleanup page in the Maintenance group shows free space, the total ready to free, and a colour bar of the disk | Must | Opening Cleanup shows "N GB available" from the same reading as the Disk page at once, and "N GB ready" when the scan ends; the UI never freezes while it scans | S2, S4 |
| FR-2 | Find git worktrees that agents made | Must | With fixture repos holding worktrees in `<repo>/.claude/worktrees`, `<root>/.worktrees/<repo>` and `~/.codex/worktrees`, every linked worktree is listed under its main repo | S2, S5, S7 |
| FR-3 | Give each worktree one state: Removable, Review, Active, In use, Orphan | Must | Fixtures prove each state: merged and clean → Removable; uncommitted or unpushed → Review; touched in the last 2 days and not merged → Active; process inside → In use; folder whose git link is broken → Orphan | S5 |
| FR-4 | Remove a Removable worktree through git, keeping its branch | Must | After removal the folder is gone, `git worktree list` no longer lists it, and the branch still exists | S5 |
| FR-5 | Prune git's records of worktrees whose folder is already gone | Should | A repo with a deleted worktree folder has no stale record after cleanup | S5 |
| FR-6 | Find Xcode build data whose project no longer exists | Must | A DerivedData folder whose `info.plist` WorkspacePath is missing is listed as safe; one whose project exists, or which records none, is not listed | S5 |
| FR-7 | Find build output in projects nobody touched for N days | Must | `node_modules`, `target`, `.build`, `build` and `.venv` are listed only next to their build file, only when git does not track them, and only when the project is idle longer than the threshold | S4, S5 |
| FR-8 | Add more known places: Gradle, CocoaPods, pip, Go, SwiftPM and Playwright caches, logs, crash reports, old installers in Downloads, agent scratch folders | Should | Each new place has a fixture test for its path and tier | S4, S5 |
| FR-9 | Each item has a tier: Safe, Rebuild or Keep | Must | Keep items show their size but cannot be ticked | S5 |
| FR-10 | Levels: Safe, Balanced and Deep tick a preset set of items; you can still tick and untick rows | Must | Switching level changes the ticked rows and the summary line; a manual tick survives a rescan | S4 |
| FR-11 | Items group into categories with icon, item count, last used, size, and a chevron to show the items | Must | Each category row reads like "Agent worktrees · 12 items · used 3 days ago · 21 GB" | S4 |
| FR-12 | One Clean up button removes the ticked items after one confirmation | Must | The dialog names the categories, item count and size, with Cancel first; the result says how much was freed or moved to the Trash | S4, S6 |
| FR-13 | Skip an item while something uses it, and say why | Must | With Xcode running, DerivedData shows a lock chip "Waits for Xcode" and is not removed; with a shell open inside a worktree, that worktree is In use | S4, S5 |
| FR-14 | Never offer protected paths | Must | `.ssh`, Keychains, provisioning profiles, keystores, `gradle.properties`, agent settings, anything git tracks and any symlink are never listed, and no listed folder contains one | S5 |
| FR-15 | An "Is it really files?" card shows swap, local APFS snapshots, a staged macOS update and booted simulators | Should | Each line shows its size or count; booted simulators get a Shut down button that asks first | S5 |
| FR-16 | Clicking the low-disk alert opens Cleanup | Should | The low-disk notification opens the window on Cleanup; other alerts keep opening the menu-bar panel | S4, S6 |
| FR-17 | The menu-bar panel shows "N GB ready to free" when it is over 5 GB | Could | The line appears after a scan and opens Cleanup | S4 |
| FR-18 | Optional AI review: your own agent CLI checks the ticked items and says keep or clean, with a reason | Could | Off by default; when on, each row shows the agent's note; the agent cannot add items, cannot untick a Keep item into Safe, and cannot remove anything; Clean up still asks first | S3, S4 |
| NFR-1 | Scanning is fast and never blocks the UI | Must | First category rows within 2 s; every worktree state within 10 s; sizes stream in, and a warm rescan of S7's 135 GB finishes in ≤ 60 s on Henrik's Mac; Cancel stops it within 1 s | S6 |
| NFR-2 | Nothing is removed without a confirmation, and each item is checked again right before removal | Must | A fixture changed after confirmation (new commit, new process, new file) is skipped and reported | S3, S5, S6 |
| NFR-3 | Every subprocess is bounded | Must | git 10 s per call, `gh` 15 s, agent 180 s; a hung child is killed and reported | S6 |
| NFR-4 | Tidegauge itself contacts no service; `gh` and the agent run only when you turn them on | Must | With both off, a scan makes no network connection | S3, S6 |
| NFR-5 | English and Norwegian Bokmål text, light and dark appearance | Must | `locale_guard` tests pass; light and dark screenshots reviewed | S6 |
| FR-19 | ⌘⌫ on Cleanup starts Clean up (with the same confirmation) | Should | On Cleanup, ⌘⌫ shows the Clean up dialog; elsewhere it does nothing | S4, Henrik 2026-11-09 |
| FR-20 | A ⌘K command bar lists every page and Cleanup action and runs the one you pick | Should | ⌘K opens it; typing filters; ↑↓ and Return run; Esc closes | S4, Henrik 2026-11-09 |
| FR-21 | Leftovers of apps that are no longer installed (`~/Library` folders named by a bundle id) | Should | A folder named for a bundle id with no installed app sharing its vendor is listed as Rebuild; `com.apple.*` and installed vendors never | S4, Henrik 2026-11-09 |
| FR-22 | Opt-in: delete the local branch after removing a merged worktree | Could | With the switch on, `git branch -d` runs after removal; an unmerged branch is refused by git and kept | S5, Henrik 2026-11-09 |
| FR-23 | Opt-in auto-clean when free space drops below a limit | Could | Off by default; when on and free space is under the limit, Safe-tier items with an undoable method are removed after a recheck, and a notification says what went | S4, S5, Henrik 2026-11-09 |
| FR-24 | Opt-in: remove worktrees as soon as their branch is merged | Could | Off by default; every 30 minutes Removable worktrees are removed after a recheck, with a notification | S5, Henrik 2026-11-09 |
| FR-25 | Agents can ask Tidegauge: a `tidegauge cleanup` command, a session-start hook and an MCP server | Could | `tidegauge cleanup status` prints free space and ready totals; `tidegauge cleanup worktrees --merged [--apply]` lists or removes Removable worktrees; `tidegauge hook session-start` warns under the limit; `tidegauge mcp` answers MCP `tools/list` and `tools/call`; Settings shows the snippets and never edits agent config | S5, Henrik 2026-11-09 |
| FR-26 | Agent review supports Claude Code, Codex, opencode, Grok, Mistral Vibe, pi and omp | Should | Each runs read-only with a fixed command; Settings lists the installed ones | Henrik 2026-11-09 |
| FR-27 | More tool data in the catalog: Android, Dart/Flutter, Electron app and editor caches, Unity, version-manager downloads; installed toolchains, emulators and Docker's disk are shown as Keep | Should | Each listed folder exists, passes the gate, and has the tier in D-17; Keep items are never ticked | disk-jockey README (read 2026-11-10), Henrik 2026-11-10 |
| FR-28 | XcodeBuildMCP workspaces untouched for 14 days are listed as Xcode build data | Should | A workspace folder with no file newer than 14 days is Safe; a newer one is not listed; it waits while Xcode builds | disk-jockey README, Henrik 2026-11-10 |
| FR-29 | "Is it really files?" also shows running Android emulators and idle Gradle and Kotlin daemons with their memory | Should | The card lists each with its memory; nothing is stopped from the card | disk-jockey README, Henrik 2026-11-10 |
| FR-30 | `tidegauge hook pre-tool` caps booted devices across agent sessions | Should | A boot over the cap (2 simulators, 1 emulator) is denied with the booted devices to reuse; booting a booted device and every other call pass silently; the hook never prints "allow" | disk-jockey README, Henrik 2026-11-10 |
| FR-31 | `tidegauge hook session-end` shuts down simulators booted during agent sessions when the last session ends | Should | With other sessions open nothing happens; at the last end only simulators booted after the first session started are shut down | disk-jockey README, Henrik 2026-11-10 |
| FR-32 | Agents are told about memory pressure | Could | Session start and a denied boot add one line when swap is over 2 GB | disk-jockey README, Henrik 2026-11-10 |
| FR-33 | A worktree analyzer lists every worktree, largest first, with its state, branch, changes and unpushed commits | Should | Every audited worktree appears once with size and reasons; Show in Finder and Open in Terminal work | Henrik 2026-11-10 |
| FR-34 | A worktree with uncommitted work can be removed by hand after typing its name; the folder goes to the Trash | Should | Without the exact name nothing happens; the recheck still runs; the folder lands in the Trash and git forgets it (`worktree prune`); in-use and orphan worktrees cannot be removed | Henrik 2026-11-10 (reverses Q-2) |
| FR-35 | Smarter auto mode: under the limit, after safe items, it trashes build output of projects idle 14 days or more, oldest first, until free space is back over the limit | Could | Off unless its own switch is on; never Rebuild tool commands, Keep, Review or in-use items; stops once over the limit | disk-jockey README, Henrik 2026-11-10 |
| FR-36 | Uninstall an app together with its leftovers | Could | Picking an app lists the app and its `~/Library` folders; after confirming, all go to the Trash; running apps, Apple apps and Tidegauge cannot be picked | Levo, Henrik 2026-11-10 |
| FR-37 | "In use by" names the real user | Should | When the process inside a folder is a shell or helper (`sleep`, `zsh`, `timeout`…), the label names the first program above it; a lone shell keeps its own name | Henrik 2026-11-10 |
| FR-38 | Selection is self-explanatory | Should | Groups show "n of m selected", the arrow sits before the icon, unpickable items show a lock with the reason, the level says what it picks and warns that it resets own ticks, the Clean up button sits by the selected total, and the confirmation names the largest items | Henrik 2026-11-10 |
| FR-39 | What will be cleaned and how far it is are visible | Should | Before a run, the header lists each group with its selected size; during a run it shows a bar, items and bytes done, the current item and a stop button, and each item and group shows its stage (waiting, now, done, skipped) | Henrik 2026-11-10 |
| CON-1 | Rust and gpui-kit, colours from `theme.rs`, tokens from `DESIGN.md` | Must | — | S6 |
| CON-2 | Folders go to the Trash; Cancel is the first button | Must | — | S6 |
| CON-3 | `Command::new` uses absolute paths | Must | — | S6 |
| CON-4 | Levo is a paid app: copy ideas only, never its name, art or text. disk-jockey is MIT: its path list may be reused with credit | Must | — | S4, S5 |
| ASM-1 | git is at `/usr/bin/git` (Command Line Tools); `gh` may be missing | — | — | S6 |
| ASM-2 | Agent worktrees live in the S2/S7 places: `<repo>/.claude/worktrees`, `<root>/.worktrees/<repo>`, `~/.codex/worktrees` | — | — | S7 |

### Traceability

| Source statement | Disposition |
|---|---|
| S2 "clean and pretty UI", "I like the Levo UI" | FR-1, FR-10, FR-11, FR-12, section 3 "Look" |
| S2 "stale worktrees and files after agent" | FR-2 to FR-7 |
| S3 AI agents, guarded and confirmed | FR-18, D-4, NFR-2 |
| S4 Auto mode | Out (D-6) |
| S4 ⌘K command bar | Later |
| S4 Quit and uninstall with leftovers | Later (Q-6) |
| S4 Memory/CPU/GPU gauges with "free it" | Partly FR-15 (simulators); the rest already exists in Apps & processes |
| S5 autopilot, agent hooks, MCP server | Out (D-6) |
| S5 quarantine | Covered by the Trash (D-7) |
| S5 squash-merged PRs via `gh` | FR-3 with Q-3 |

## 2. Scope

- **In:** FR-1 to FR-39, NFR-1 to NFR-5. Henrik chose "everything" on 2026-11-09 (D-13) and added FR-27–FR-36 on 2026-11-10.
- **Out:** nothing.
- **Later:** nothing.

## 3. Conceptual design

### Purpose

Show, on one page, what is filling the disk because of agent work and
development tools, and remove what is provably safe after you confirm.
It does not clean on its own, it never deletes a branch, it never empties
the Trash, and it never lets an AI agent remove anything.

### Look

The page copies Levo's layout, drawn with Tidegauge's tokens and widgets.

```
┌ Cleanup ──────────────────────────────────────────── ⓘ ┐
│  61 GB available                         52 GB ready   │
│  ▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▮▯▯▯▯▯▯▯▯▯▯▯▯▯▯▯     │  used · ready · free
│                                                         │
│  [ Safe | Balanced | Deep ]                             │
│  A balanced cleanup frees 52 GB: Agent worktrees,       │
│  Xcode build data and 3 more.                           │
│                                                         │
│  Is it really files?  Swap 6 GB · 3 snapshots · 2 sims ›│
│                                                         │
│  Ready to free                           Sort: Size ▾   │
│  ☑ ⎇  Agent worktrees      31 items · 3 days ago  38 GB ›│
│  ☑ ⚒  Xcode build data      9 items · 1 week ago  9 GB ›│
│  ☐ ▣  Project build output 14 items · 2 months   12 GB ›│
│  ☑ ⧉  Package caches        5 items · today       3 GB ›│
│  🔒   Simulators            Waits for Xcode       7 GB ›│
│  —    Kept                  Review worktrees …    6 GB ›│
│                                                         │
│  Checked 2 min ago · Rescan        [ Clean up 52 GB ⌘⌫ ]│
└─────────────────────────────────────────────────────────┘
```

- The header and bar use the Disk colour (amber) for "used", a lighter tint
  for "ready", and the track for "free".
- A category row expands to its items. A worktree item shows repo, branch,
  state chip (Removable, Review, Active, In use, Orphan), last commit time and
  size, plus Show in Finder.
- Keep items sit in a dimmed "Kept" group with the reason, never a checkbox.
- With nothing to free, the page shows the standard empty-state card.

### Context map

```
          you ──confirm──▶ ┌──────────── Tidegauge ─────────────┐
                           │ Cleanup page ◀── Candidate finders │
  low-disk alert ─click──▶ │      │                │           │
                           │      ▼                ▼           │
                           │ Safety gate ──▶ Remover           │
                           │      ▲                            │
                           │ Agent reviewer (opt-in)           │
                           └──┬──────┬─────────┬──────────┬────┘
      ═══ trust boundary ═════╪══════╪═════════╪══════════╪═════
                              ▼      ▼         ▼          ▼
                        file system  git   gh (opt-in)  agent CLI (opt-in)
                        Trash, simctl, tmutil            claude / codex
```

Top threats:
- **File system.** Tampering: a symlink or a swapped folder between scan and
  removal. Mitigation: no links, canonical paths, and a re-check right before
  removal (NFR-2).
- **git and gh.** Spoofing: a wrong "merged" answer. Mitigation: local git is
  the main check, and `gh` can only confirm a squash merge (Q-3).
- **Agent CLI.** Prompt injection through a file name or commit message, and
  disclosure of paths to a provider. Mitigation: the agent sees only listed
  items and answers by item ID. Tidegauge drops any ID it did not send.
  The review runs only after you opt in, and the first run says which
  provider will see the paths.

### Capabilities

| Capability | FR |
|---|---|
| Show disk space and what is ready | FR-1, FR-11 |
| Audit agent worktrees | FR-2, FR-3, FR-5 |
| Find leftover build data and caches | FR-6, FR-7, FR-8 |
| Explain non-file pressure | FR-15 |
| Choose what goes | FR-9, FR-10 |
| Remove safely | FR-4, FR-12, FR-13, FR-14 |
| Get a second opinion from your agent | FR-18 |
| Reach the page when it matters | FR-16 |

### Components

| Component | Responsibility | Requirements |
|---|---|---|
| Cleanup page | Draw header, bar, levels, rows; collect ticks; confirm | FR-1, FR-10, FR-11, FR-12, NFR-5 |
| Worktree auditor | Find worktrees, read git state, give each a state | FR-2, FR-3, FR-5 |
| Leftover finder | Orphan DerivedData, stale build output, extended cache catalog | FR-6, FR-7, FR-8 |
| Pressure check | Swap, snapshots, staged update, booted simulators | FR-15 |
| Safety gate | Tier, protected paths, lock checks, re-check before removal | FR-9, FR-13, FR-14, NFR-2 |
| Remover | Trash, `git worktree remove`, tool commands; report freed bytes | FR-4, FR-5, FR-12, NFR-3 |
| Agent reviewer | Run your agent CLI read-only on the ticked list and parse its notes | FR-18, NFR-3, NFR-4 |

### Domain concepts

- **Candidate:** one removable thing (a worktree, a folder, a tool cache) with
  paths, size, last used, tier, method and evidence.
- **Category:** a group of candidates shown as one row (Agent worktrees, Xcode
  build data, and so on).
- **Tier:** Safe (pure cache or provably orphaned), Rebuild (costs a download
  or build), Keep (holds real data: shown only).
- **Level:** a preset choice of tiers: Safe = Safe tier; Balanced = Safe plus
  Rebuild items idle past the threshold; Deep = Safe plus all Rebuild.
- **Lock:** a reason a candidate waits right now ("Xcode is open").
- **Worktree state:** Removable, Review, Active, In use, Orphan (FR-3).

## 4. Logical design

### Component contracts

- **Worktree auditor.** `audit(roots) → [Worktree]`. Async and best-effort: a
  repo that fails is reported, not fatal. It depends on git (and `gh` when it
  is on).
- **Leftover finder.** `find(roots, thresholds) → [Candidate]`. Async,
  bounded by the `storage.rs` limits. It reuses `caches::folder_bytes` for
  sizes.
- **Pressure check.** `read() → Pressure`. Each part is optional and
  independent.
- **Safety gate.**
  - `classify(candidate) → tier + lock`: pure, apart from the process list.
  - `recheck(candidate) → Ok | Refused(reason)`: synchronous. It must pass
    within 1 s before each removal.
- **Remover.** `remove(plan) → Report`. It runs one candidate at a time,
  calling `recheck` before each. The report lists freed bytes, items moved to
  the Trash, items skipped with their reason, and errors.
- **Agent reviewer.** `review(ticked) → [Note]`. Async, opt-in, 180 s limit.
  A note is {id, keep|clean, reason}.

### Domain model

- **Worktree:**
  - repo, path, branch, head, base branch, upstream
  - dirty, unpushed count, merged (local | gh | no)
  - last commit time, last file change, processes inside
  - state
- **Candidate:**
  - id, category, tier, method (Trash | GitWorktree | Tool), paths
  - bytes, last_used, lock, evidence, note (from the agent)
- **Invariants:**
  - A Keep candidate is never in a plan.
  - A Review or In use worktree is a Keep candidate.
  - Every path is inside the home folder or the user's agent scratch in
    `/private/tmp`, owned by the user, and not a link.

### Data ownership

| Datum | Owner | Readers |
|---|---|---|
| Scan roots, thresholds, agent choice | Settings | All finders, Agent reviewer |
| Worktree list and states | Worktree auditor | Safety gate, Page |
| Candidates and tiers | Safety gate | Page, Remover, Agent reviewer |
| Ticks and level | Cleanup page | Remover |
| Agent notes | Agent reviewer | Page |
| Free space | `metrics::disk_stats` (existing) | Page |

### Integration

| Link | Direction | Kind |
|---|---|---|
| Page → finders | Page commands | Request/response off the UI thread (`smol::unblock`), results streamed per category |
| Worktree auditor → git | Auditor commands | Bounded subprocess: `git -C <repo> worktree list --porcelain`, `status --porcelain`, `rev-list --count @{u}..`, `merge-base --is-ancestor`, `log -1 --format=%ct` |
| Worktree auditor → gh | Auditor commands, opt-in | `gh pr list --head <branch> --state merged --json number` |
| Remover → file system | Remover commands | `NSFileManager trashItem` (existing `move_to_applications::trash`) |
| Remover → git | Remover commands | `git worktree remove <path>` (no `--force`), `git worktree prune` |
| Agent reviewer → agent CLI | Reviewer commands, opt-in | One bounded run with a JSON list on stdin and JSON notes on stdout (detailed in slice 6) |
| Notification click → window | macOS → app | The click's userInfo names the alert; `low_disk` opens Cleanup |

### Scenario walkthroughs

**A. Clean up stale worktrees (FR-1–4, 9–14, NFR-1–2)**
1. You open Cleanup. The page shows free space at once (`disk_stats`) and
   starts `audit` and `find`.
2. The auditor lists worktrees per repo, reads their git state and gives each
   a state. In use comes from the process list (the cwd is inside the folder).
3. The safety gate sets tiers: Removable → Safe, Review and In use → Keep.
4. The page draws the categories with Balanced ticked and the summary line.
5. You press Clean up. The dialog names "31 agent worktrees, 9 Xcode build
   folders, 52 GB", with Cancel first. You confirm.
6. For each candidate, the Remover calls `recheck`. git state, processes and
   the folder identity must match the scan. Then `git worktree remove`.
7. The report reads "Freed 49 GB. 2 skipped: a process started inside
   tidegauge/feat-x." The page rescans.

**B. A worktree changes after you confirm (NFR-2)**
1. While the dialog is open, an agent commits in a worktree you ticked.
2. `recheck` sees the head differs from the scan and refuses.
3. That item is skipped, the rest go, and the report names it.

**C. Ask your agent (FR-18, NFR-3, NFR-4)**
1. In Settings → Cleanup, you turn on "Ask an agent before cleaning" and pick
   Claude Code. The first run shows which provider will see the paths.
2. On Cleanup you press "Ask Claude Code". The reviewer sends only the ticked
   candidates (id, path, category, size, git state, last used). The agent may
   read those paths but not write them.
3. It returns notes. Tidegauge drops unknown IDs. A "keep" note unticks the
   row and shows the reason; a "clean" note only adds a check mark. It never
   ticks a Keep item.
4. You press Clean up, and the usual confirmation and re-checks apply.

**D. Low disk (FR-16)**
1. Free space stays under 10 GB for 60 s, and the alert fires (existing).
2. You click it. The window opens on Cleanup, and a scan starts.

### NFR mechanisms

| NFR | Mechanism | Sufficiency argument | Numeric |
|---|---|---|---|
| NFR-1 | Cheap checks first, sizes after; per-category streaming; size cache keyed by folder mtime + inode | 86 worktrees × ~5 git calls at ~20 ms ≈ 9 s, in parallel per repo (11 repos) ≈ 1–2 s. Sizing 135 GB of mostly build output is the slow part: one `du` walk took about 3 min, so the first scan shows each size as it lands, and later scans reuse cached sizes for unchanged folders | yes |
| NFR-2 | `recheck` compares the head, dirty flag, process list, inode and mtime against the scan | Each re-check is ≤ 4 git calls ≈ 100 ms per item, so 40 items ≈ 4 s | yes |
| NFR-3 | `bounded::Probe` / `packages::run_command` with per-call limits | Existing runner kills on timeout and caps output | yes |
| NFR-4 | `gh` and the agent run only behind settings that default to off | No other network code is added | no |
| NFR-5 | `locale::text` for every string; existing guard tests | — | no |

### Risk register

| Risk | Impact | Mitigation |
|---|---|---|
| A worktree is wrongly called Removable | Lost work | Removable requires clean, no unpushed commits and merged; `git worktree remove` without `--force` refuses dirty trees anyway; the branch stays |
| Ignored files in a clean worktree (`.env`, local data) are lost | Lost local secrets | Before removal, list ignored files that are not build output; if any exist, the worktree is Review (Q-8) |
| Scanning Downloads or Documents triggers macOS privacy prompts | Annoying prompts | Downloads only in Deep and only after you tick it once; never Documents or Desktop |
| Sizing is slow on huge trees | Page feels slow | Streaming, cached sizes, Cancel |
| An agent's note is wrong or injected | A bad keep or clean hint | Notes are advice only: IDs are checked, Keep tier is untouchable, and you still confirm |
| `gh` missing or signed out | Squash merges not seen | Those worktrees stay Review; the page says "Install gh to detect squash merges" |

## 5. Decisions

### D-1: A new Cleanup page; Folder sizes keeps the explorer (Proposed, 2026-11-09)
Context: the caches list lives on the Storage page today (S6).
Options: (a) grow the Storage page; (b) a new Maintenance page that takes over
the caches list.
Choice: (b). Folder sizes stays a pure explorer, and Cleanup becomes the one
place that removes things.
Consequences: the caches section moves; the sidebar order test changes; ⌘
numbers shift by one.

### D-2: Worktrees are removed by git, never by moving the folder (Accepted, 2026-11-09)
Context: trashing a worktree folder leaves git's record and a dangling branch
link.
Choice: `git worktree remove` without `--force`. Only an Orphan folder (its git
link is already broken) goes to the Trash.
Consequences: removed worktrees are not in the Trash. That is safe, because
only clean, pushed, merged trees qualify, and their commits stay on the branch.

### D-3: Three tiers and three levels (Proposed, 2026-11-09)
Context: Levo has levels; disk-jockey has tiers. Tiers say what an item is,
and levels say how far you go.
Choice: tiers Safe, Rebuild and Keep; levels Safe, Balanced and Deep as
defined in section 3. Balanced is the default.

### D-4: The AI agent advises; it never acts (Proposed, 2026-11-09)
Context: S3 allows AI help if it is guarded. Tidegauge has promised never to
call a provider.
Options: (a) no agent; (b) the agent picks and removes; (c) the agent only
writes notes on a list Tidegauge built.
Choice: (c), opt-in, using your own installed CLI with read-only permissions.
Tidegauge itself still calls no provider.
Consequences: the README's privacy line gains "unless you turn on agent review".

### D-5: Merged = local git first, `gh` optional (Proposed, 2026-11-09)
Context: most agent PRs are squash-merged, which local git cannot see.
Choice: local ancestry check always. `gh` is opt-in and only confirms a merge.

### D-6: No automatic cleaning (Proposed, 2026-11-09)
Context: Levo's Auto mode and disk-jockey's autopilot remove files without you
watching.
Choice: no auto mode. The low-disk alert opens Cleanup instead (FR-16).

### D-7: The macOS Trash is the undo (Accepted, 2026-11-09)
Context: disk-jockey has its own quarantine; Tidegauge already trashes folders.
Choice: Trash for folders; no extra quarantine. Tool caches (`brew cleanup`,
`simctl`) cannot be undone, and the dialog says so (existing behaviour).

### D-8: git runs read-only and hardened (Accepted, 2026-11-09)
Context: agents are working in these repos while Tidegauge reads them, and a
repo's own config can name programs git will run (fsmonitor, hooks).
Choice: every read call runs `/usr/bin/git` with `GIT_OPTIONAL_LOCKS=0` (so
`status` never takes `index.lock` from a busy agent), `GIT_TERMINAL_PROMPT=0`,
`LC_ALL=C`, and `-c core.fsmonitor=false -c core.hooksPath=/dev/null`. Tidegauge
never fetches; "merged" uses the remote refs as last fetched, and the page
says so.
Consequences: a branch merged on GitHub but not fetched since shows as Review
unless `gh` is on.

### D-9: Scans run on worker threads with shared progress (Accepted, 2026-11-09)
Context: the codebase has two patterns: `smol::unblock` with one result
(caches) and a thread with `Arc<Mutex<Progress>>` that the view reads while
drawing (folder sizes). Cleanup needs streaming and Cancel.
Choice: the folder-sizes pattern: one coordinator thread, four git workers and
two sizing workers, all writing into `Arc<Mutex<ScanState>>`, with an
`AtomicBool` cancel. The view polls it while a scan runs, as Folder sizes does.

### D-10: Agent CLIs are pinned per tool, read-only, and found at fixed paths (Proposed, 2026-11-09)
Context: CON-3 demands absolute paths; the agent must not write (D-4).
Choice: Claude Code runs `claude -p` with only `Read`, `Glob` and `Grep`
allowed; Codex runs `codex exec --sandbox read-only`. The binary is looked up
in a fixed list of install folders and shown in Settings; there is no `PATH`
search. Exact flags are checked against the installed versions when slice 6 is
built, and recorded as a delta if they differ.

### D-11: A worktree touched in the last days is not Removable on local git alone (Accepted, 2026-11-09)
Context: building slice 1 showed that a worktree made minutes ago, before its
first commit, sits on the base branch and so also reads as "merged".
Choice: when local git is the only evidence of a merge, a worktree touched
within `cleanup_active_days` is Active. A merge that `gh` confirms for the
exact HEAD is Removable at once.
Consequences: a branch merged today waits up to two days unless `gh` is on.
This changes classification rules 7 and 8 in section 9.

### D-12: Orphan worktrees are shown, never removed (Accepted, 2026-11-09)
Context: review found that a folder whose git link is broken may belong to a
repo that was only moved, renamed, or sits on an unmounted disk, and its
uncommitted work can't be checked without that repo.
Choice: Orphan is Keep tier with Show in Finder; Tidegauge never trashes it.
Consequences: supersedes the Orphan → Safe part of FR-3's tiering. git's own
`prunable` records are still pruned, except on `/Volumes`.

### D-13: Everything ships, the risky parts off by default (Accepted, 2026-11-09, Henrik)
Context: Henrik asked for every slice and every Later and Out item.
Choice: auto-clean, clean-after-merge, branch deletion, hooks and MCP are
built, each behind a switch that defaults to off. Supersedes D-6 and the
"never in v1" answer to Q-5.

### D-14: Automatic actions take only Safe, undoable items (Accepted, 2026-11-09)
Context: FR-23 and FR-24 run without a dialog.
Choice: they take only Safe-tier items whose method is the Trash or
`git worktree remove` of a merged, clean, pushed, unused worktree; never a
tool command (Homebrew, Go, simctl), never Rebuild or Keep. Every item gets
the same recheck, at most once every 30 minutes, and a notification lists
what went.

### D-15: Hooks and MCP live in the Tidegauge binary; Tidegauge never edits agent config (Accepted, 2026-11-09)
Choice: `tidegauge cleanup …`, `tidegauge hook session-start` and
`tidegauge mcp` run without the window or the single-instance lock.
Settings shows the snippets with a copy button; you paste them into your
agent's config yourself. The MCP server and the CLI list freely; removing
needs `apply: true` (or `--apply`) and the switch "Let agents remove merged
worktrees", off by default, and takes only Removable worktrees.

### D-16: Agent CLI commands (Accepted, 2026-11-09; flags read from each tool's `--help` on Henrik's Mac)
| Agent | Command (prompt) | Read-only by |
|---|---|---|
| Claude Code | `claude -p --tools Read Glob Grep --disallowedTools …` (stdin) | tool allowlist |
| Codex | `codex exec --sandbox read-only --ephemeral -o FILE -` (stdin) | sandbox |
| opencode | `opencode run --agent plan --format default PROMPT` | plan agent denies edits |
| Grok | `grok -p PROMPT --permission-mode plan --disable-web-search --no-subagents --output-format plain --max-turns 20 --add-dir …` | plan mode |
| Mistral Vibe | `vibe --enabled-tools read_file --enabled-tools grep --output text --max-turns 20 --prompt-file FILE --add-dir …` | headless tool allowlist |
| pi | `pi -p --no-session --no-extensions --no-mcp --tools read,grep,find,ls -- PROMPT` | tool allowlist |
| omp | `omp -p --no-session --no-lsp --tools read,grep,find PROMPT` | tool allowlist |

### D-17: Tiers for the new catalog entries (Accepted, 2026-11-10)
Context: FR-27 adds data that ranges from pure caches to installed toolchains.
Choice: download and build caches are Safe or Rebuild; anything that holds
something you installed or made (emulators, toolchains, Docker's disk) is Keep.
Consequences: Docker is never touched; its own Clean up is the way.

### D-18: Uncommitted worktrees go to the Trash, not `--force` (Accepted, 2026-11-10; supersedes the Q-2 draft answer)
Context: disk-jockey discards uncommitted changes after a typed "yes".
Choice: the folder moves to the Trash, then `git worktree prune` drops the
record. Changes can be dragged back; commits stay on the branch.
Consequences: space is freed when the Trash is emptied; the branch stays.

### D-19: The auto ladder only trashes (Accepted, 2026-11-10; extends D-14)
Context: FR-35. Choice: the second step takes only build output with the
Trash method, from projects idle 14 days or more, oldest first.

### D-20: XcodeBuildMCP workspaces by age (Accepted, 2026-11-10)
Context: on Henrik's Mac the workspace folders hold no project record, so
"project gone" cannot be proven. Choice: 14 days with no newer file.

## 6. Coverage

| Requirement | Component | Slice |
|---|---|---|
| FR-2, FR-3, FR-4, FR-13 (worktrees), FR-14, NFR-2, NFR-3 | Worktree auditor, Safety gate, Remover | 1 |
| FR-1, FR-9, FR-10, FR-11, FR-12, NFR-1, NFR-5 | Cleanup page | 2 |
| FR-5, FR-6, FR-7, FR-13 (Xcode, Gradle) | Leftover finder, Safety gate | 3 |
| FR-8, FR-15 | Leftover finder, Pressure check | 4 |
| FR-16 | Notifications | 5 |
| FR-18, NFR-4 | Agent reviewer | 6 |

## 7. Build plan

| Slice | What it delivers | Requirement IDs | Status |
|---|---|---|---|
| 1 | Worktree auditor, safety gate and git removal, behind a plain list on a new Cleanup page: find, classify, remove one worktree with confirmation. This is the riskiest contract (lost work) | FR-2, FR-3, FR-4, FR-13, FR-14, NFR-2, NFR-3 | Done |
| 2 | The Levo-style page: header, colour bar, levels, category rows with checkboxes, summary line, one Clean up button; the caches list moves in. New widgets: checkbox, stacked capacity bar | FR-1, FR-9, FR-10, FR-11, FR-12, NFR-1, NFR-5 | Done |
| 3 | Leftovers: orphan DerivedData, stale build output, `git worktree prune`, Xcode and Gradle locks | FR-5, FR-6, FR-7, FR-13 | Done |
| 4 | More caches from disk-jockey's list (with credit), and the "Is it really files?" card | FR-8, FR-15 | Done |
| 5 | Low-disk alert opens Cleanup | FR-16 | Done |
| 6 | Agent review (opt-in) | FR-18, NFR-4 | Done |
| 7 | ⌘⌫ and the menu-bar "ready to free" line | FR-17, FR-19 | Done |
| 8 | ⌘K command bar | FR-20 | Done |
| 9 | App leftovers | FR-21 | Done |
| 10 | Branch deletion, auto-clean, clean after merge | FR-22, FR-23, FR-24 | Done |
| 11 | `tidegauge cleanup`, session-start hook, MCP server; seven agents | FR-25, FR-26 | Done |
| 12 | Catalog additions, XcodeBuildMCP workspaces, emulators and daemons in the pressure card | FR-27, FR-28, FR-29 | Done |
| 13 | Device-cap, session-end and memory hooks | FR-30, FR-31, FR-32 | Done |
| 14 | Worktree analyzer with manual removal to the Trash | FR-33, FR-34 | Done |
| 15 | Auto ladder | FR-35 | Done |
| 16 | Uninstall an app with its leftovers | FR-36 | Done |
| 17 | Clearer selection and real in-use names | FR-37, FR-38 | Done |
| 18 | Visible plan and live progress | FR-39 | Done |

Each slice ships as its own PR with fixture tests (temporary git repos made in
the test, never real repos), `cargo test`, and a light and dark screenshot
for UI slices.

## 8. Open questions

| ID | Question | Blocks | Draft answer |
|---|---|---|---|
| Q-1 | Which folders to scan for repos? | FR-2, FR-7 | `~/Dev`, `~/Developer`, `~/Projects`, `~/code` when present, plus every folder an agent session ran in; editable in Settings |
| Q-2 | May Review worktrees (uncommitted work) be removed after a typed confirmation, like disk-jockey? | FR-3 | No in v1: show them with Show in Finder and Open in Terminal |
| Q-3 | Turn `gh` on by default? | FR-3 | Off; one switch in Settings |
| Q-4 | Which agents for review in v1? | FR-18 | Claude Code and Codex |
| Q-5 | Delete the branch after removing a merged worktree? | FR-4 | Never in v1 |
| Q-6 | Uninstall apps with their leftover files? | Later | Separate piece of work on the Packages page |
| Q-7 | Idle thresholds | FR-3, FR-7 | Worktree Active window 2 days; build output 30 days (Balanced) |
| Q-8 | A clean worktree holds ignored non-build files (for example `.env`): Review or Removable? | FR-3 | Review |
| Q-9 | Approve the Out list (auto-clean, hooks, MCP, branch deletion)? | Scope | Yes |

## 9. Detailed design

All six slices are designed now so they can be built in order (see the working
assumptions at the top). Each slice still updates this section if the code
must differ.

### Conventions for every slice

- **Modules.** New code lives in `src/cleanup/`. The page is
  `src/cleanup_panel.rs`, beside `tools_panel.rs`.

  | Module | Job |
  |---|---|
  | `cleanup/mod.rs` | `Candidate`, `Category`, `Tier`, `Method`, `Lock`, `Level`, `ScanState`, `scan()` coordinator |
  | `cleanup/git.rs` | Hardened, bounded git runner (D-8) and porcelain parsers |
  | `cleanup/worktrees.rs` | Worktree auditor (FR-2, FR-3, FR-5) |
  | `cleanup/gate.rs` | Protected paths, locks, tiers, `recheck` (FR-9, FR-13, FR-14, NFR-2) |
  | `cleanup/remove.rs` | Remover and its report (FR-4, FR-12) |
  | `cleanup/leftovers.rs` | Orphan DerivedData, build output, catalog (FR-6, FR-7, FR-8) — slice 3–4 |
  | `cleanup/pressure.rs` | Swap, snapshots, simulators (FR-15) — slice 4 |
  | `cleanup/agent.rs` | Agent reviewer (FR-18) — slice 6 |

- **Errors.** Finders never fail the whole scan. Each one adds a `Skipped
  { place, reason }` to `ScanState.skipped`, and the page shows these behind
  "N places skipped", as Folder sizes does today.
- **Subprocesses.**
  - Absolute paths only (CON-3).
  - Every call goes through `packages::run_command(cmd, timeout)` (make it
    `pub(crate)` if it is not). `bounded::Probe` allows one child at a time,
    and the git workers need four in parallel.
  - Timeouts: git 10 s, `gh` 15 s, `simctl` 30 s, `tmutil` 10 s, agent 180 s.
- **Text.** Every string goes through `locale::text(en, nb)`. Sizes use the
  existing formatter, in the 1024-based units the app shows.
- **IDs.** A candidate's ID is its canonical path, plus `#tool:<name>` for
  tool caches. It is stable across rescans, which keeps manual ticks (FR-10).
- **Tests.**
  - Unit tests sit next to the code.
  - Git fixtures are made in a temp folder by a helper,
    `cleanup::test_support::repo()`, which runs `git init`, commits, adds
    worktrees, and sets up a bare "origin" to fake pushes and merges.
  - Tests never touch real repos or `~` (CONTRIBUTING).
  - `cargo test --locked cleanup::` runs them all.

### Slice 1 — Worktree auditor and safe removal

Delivers FR-2, FR-3, FR-4, FR-13 (process locks), FR-14, NFR-2, NFR-3, on a
plain list page.

**Interfaces** (`cleanup/mod.rs`)

```rust
pub enum Tier { Safe, Rebuild, Keep }
pub enum Method { Trash, GitWorktree { main: PathBuf }, Tool(ToolAction) }
pub enum WorktreeState { Removable, Review(Why), Active, InUse(String), Orphan }
pub enum Why { Uncommitted(u32), Unpushed(u32), IgnoredFiles(Vec<String>), NotMerged, Detached, Locked }
pub struct Candidate {
    pub id: String,                 // canonical path
    pub category: Category,         // AgentWorktrees in slice 1
    pub title: String,              // "tidegauge · feat/cleanup"
    pub paths: Vec<PathBuf>,
    pub bytes: Option<u64>,         // None while measuring
    pub last_used: Option<i64>,     // unix seconds
    pub tier: Tier,
    pub method: Method,
    pub lock: Option<Lock>,         // reason it waits now
    pub state: Option<WorktreeState>,
    pub fingerprint: Fingerprint,   // what recheck compares
    pub note: Option<AgentNote>,    // slice 6
}
pub struct Fingerprint { head: Option<String>, status_hash: u64, ino: u64, dev: u64 }
pub fn scan(roots: Vec<PathBuf>, cfg: ScanConfig, cancel: Arc<AtomicBool>) -> Arc<Mutex<ScanState>>;
```

**Literal example: one audited worktree**

```json
{ "id": "/Users/henrik/Dev/Repos/brev/.claude/worktrees/fix-auth-3f2a",
  "category": "AgentWorktrees", "title": "brev · claude/fix-auth-3f2a",
  "bytes": 2147483648, "last_used": 1793865600, "tier": "Safe",
  "method": {"GitWorktree": {"main": "/Users/henrik/Dev/Repos/brev"}},
  "lock": null, "state": "Removable" }
```

**Discovery (FR-2)**

1. Roots come from Q-1:
   - `~/Dev`, `~/Developer`, `~/Projects` and `~/code` when they exist;
   - plus `settings.cleanup_roots`;
   - plus each distinct `agents::Session.folder` seen since launch, walked up
     to its git top.
2. In each root, find main repos: folders up to 3 levels deep that hold a
   `.git` **folder**. Skip `node_modules`, `.build`, `target`, dot-folders
   other than `.worktrees`, and symlinks.
3. For each main repo, run `git worktree list --porcelain` and parse the
   `worktree`, `HEAD`, `branch`, `detached`, `locked` and `prunable` records.
   The first record is the main checkout, which is never a candidate.
4. Find orphans. Walk `<repo>/.claude/worktrees/*`, `<root>/.worktrees/*/*`
   and `~/.codex/worktrees/*/*`. A folder whose `.git` **file** names a
   `gitdir:` that no longer exists, and which no main repo lists, is Orphan.
5. A Codex worktree whose main repo lies outside the roots is found by reading
   its `.git` file (`gitdir: <main>/.git/worktrees/<name>`) and adding that
   main repo.

**Classification (FR-3).** For each linked worktree, in order; the first match
wins.

| # | Test (git calls per D-8) | State | Tier |
|---|---|---|---|
| 1 | `prunable` in the list, or the folder is missing | (handled by prune, slice 3) | — |
| 2 | `locked` in the list | Review(Locked) | Keep |
| 3 | A process of this user has its cwd inside the path (`processes::list_pids` + `proc_cwd`), or an agent session folder is inside it | InUse(app name) | Keep |
| 4 | `git status --porcelain=v1 --untracked-files=normal` is not empty | Review(Uncommitted n) | Keep |
| 5 | `git status --porcelain=v1 --ignored=matching` lists ignored entries other than the build-output names (Q-8) | Review(IgnoredFiles) | Keep |
| 6 | Upstream set and `git rev-list --count @{u}..HEAD` > 0 | Review(Unpushed n) | Keep |
| 7 | `git merge-base --is-ancestor HEAD <base>` is true, or `gh` (when on) reports a merged PR whose `headRefOid` equals HEAD | Removable | Safe |
| 8 | Newest of the last commit time and the folder's top-level mtimes is under `cleanup_active_days` (2) | Active | Keep |
| 9 | Otherwise | Review(NotMerged or Detached) | Keep |

- `<base>` is `git symbolic-ref --short refs/remotes/origin/HEAD`, else
  `origin/main`, else `main`, else `master`.
- The build-output names are `node_modules`, `target`, `.build`, `build`,
  `dist`, `.next`, `.turbo`, `.gradle`, `.venv`, `__pycache__`, `DerivedData`
  and `.cache`.
- `gh` runs as
  `gh pr list --repo <origin> --head <branch> --state merged --json number,headRefOid --limit 5`,
  from a fixed list of paths: `/opt/homebrew/bin/gh`, `/usr/local/bin/gh`.

**Protected paths (FR-14).** `gate::is_protected(path)` is true when the path
is any of these, or contains one:

- `~/.ssh`, `~/Library/Keychains`, `~/Library/MobileDevice/Provisioning Profiles`
- `*.keystore`, `*.jks`, `gradle.properties`
- `~/.claude/settings*.json`, `~/.claude/CLAUDE.md`, `~/.codex/config.toml`,
  `~/.config/gh`
- the home folder itself, and any main checkout

The rule is checked for every path before it reaches a list and again in
`recheck`. A worktree containing `.env*` is caught earlier by rule 5.

**Recheck (NFR-2).** `gate::recheck(&Candidate) -> Result<(), Refusal>`, run
right before removal. It refuses when:

- the folder's dev or inode changed, or the path is now a symlink;
- `rev-parse HEAD` differs from the fingerprint;
- `status --porcelain` is no longer empty, or its hash changed;
- a process now has its cwd inside;
- the path is protected.

Refusal reasons are localized strings: "Changed since the scan: new commit".

**Removal (FR-4).** `remove::run(plan: Vec<Candidate>, cancel) -> Report`:

- For each candidate in order, run `recheck`, then the method.
  `GitWorktree`: `git -C <main> worktree remove <path>`, never `--force`.
  `Trash`: `move_to_applications::trash`. Then `statfs` free space again.
- `Report { freed_bytes, trashed_bytes, removed: Vec<String>,
  skipped: Vec<(String, String)>, errors: Vec<(String, String)> }`.
  `freed_bytes` is the measured gain in free space, not the sum of sizes.
- If git refuses, for example because a file appeared, the item goes to
  `skipped` with git's first stderr line.
- Each removal is its own step. A failure in the middle leaves the rest
  untouched, and no retry happens on its own.

**Page in slice 1.**

- Add `Page::Cleanup` (`label` "Cleanup"/"Opprydding", key `cleanup`, icon
  `icons/sparkles.svg`, or the nearest existing icon) as the first item in the
  Maintenance group (`ui.rs` `sidebar_groups`).
- Also update `Destination::from_key`, `panel()`, `chrome()`, `about_notes()`
  and `open_page`, and the sidebar order test.
- The list is grouped by repo: title, state chip, size, last used and a
  Remove button, which only Removable rows have.
- Remove uses `window.prompt(Warning, …)` with Cancel first, then the
  report card.

**Settings (slice 1).**

- `cleanup_roots: Vec<PathBuf>` (default empty)
- `cleanup_active_days: u32 = 2`
- `cleanup_use_gh: bool = false`

All three use `#[serde(default)]`.

**Failure modes**

- git times out: the repo is listed as skipped ("git did not answer in
  10 s"), and its worktrees are not shown.
- git is missing: the page shows an empty state, "Install the Command Line
  Tools to find worktrees".
- `gh` fails or is not signed in: rule 7 falls back to local git, and a
  one-line note shows.
- Process inspection fails (`list_pids` is empty): every worktree becomes
  InUse("unknown"). This fails closed, as disk-jockey does.

**Tests**

| AC | Test | Type |
|---|---|---|
| FR-2 | `worktrees::finds_claude_codex_and_dot_worktrees` | integration (fixture) |
| FR-3 | `classify_removable_when_merged_and_clean`, `…_review_when_dirty`, `…_review_when_unpushed`, `…_review_when_env_ignored`, `…_active_when_recent`, `…_in_use_when_cwd_inside`, `…_orphan_when_gitdir_missing`, `…_locked` | integration |
| FR-4 | `remove_keeps_branch_and_drops_listing` | integration |
| FR-14 | `gate::protected_paths_never_listed`, `…_parent_of_protected_refused` | unit |
| NFR-2 | `recheck_refuses_after_new_commit`, `…_after_new_file`, `…_after_inode_swap` | integration |
| NFR-3 | `git_runner_kills_after_timeout` (fake git script that sleeps) | unit |
| D-8 | `git_status_takes_no_index_lock` (asserts no `index.lock` is created while `status` runs) | integration |

### Slice 2 — The Levo-style page

Delivers FR-1, FR-9, FR-10, FR-11, FR-12, NFR-1, NFR-5.

**New widgets** (`controls.rs` / `ui.rs`, tokens only)

- `controls::checkbox(id, state: Check, dark, on_toggle)`, where
  `Check = Off | On | Mixed`.
  - 14 px box, `RADIUS_CHIP`, `accent` fill when On, a dash for Mixed.
  - Disabled look for Keep rows.
  - Keyboard: Space toggles it.
- `ui::stacked_bar(parts: &[(f64, Hsla)], dark)`: the existing
  `capacity_bar`, made public and taught several segments.
  - Height 10 px.
  - Segments: used = Disk colour (amber), ready = amber at 45% alpha, free =
    track.
- `ui::summary_line(level, cats)` returns text with bold runs: "A balanced
  cleanup frees 52 GB: Agent worktrees, Xcode build data and 3 more."

**Layout** follows section 3 "Look". The order from top:

1. Header: free space at `TEXT_TITLE`, and "N GB ready" in success colour on
   the right.
2. Stacked bar.
3. Level segment group: Safe | Balanced | Deep.
4. Summary line.
5. Pressure card (slice 4; hidden until then).
6. "Ready to free" `list_card`, with Sort (Size, Last used, Name) on the
   right.
7. The "Kept" group, dimmed.
8. Footer: "Checked 2 min ago · Rescan", and `button_accent` "Clean up 52 GB"
   with ⌘⌫.

**Category row** (`list_row` with a leading checkbox):

- Mixed when some items are ticked; a chevron expands the items.
- Meta line: "12 items · used 3 days ago". Sizes are right-aligned.
- While a size is unknown, it shows "Measuring…", and the row cannot be
  ticked yet.
- Expanded item rows show title, state chip, size and Show in Finder.

**Levels (FR-10)**

| Level | Ticks |
|---|---|
| Safe | Every Safe candidate |
| Balanced (default) | Safe, plus Rebuild candidates idle ≥ `cleanup_build_idle_days` (30) |
| Deep | Safe, plus every Rebuild candidate idle ≥ 7 days |

Manual ticks are kept in `overrides: HashMap<id, bool>` and win over the
level. They survive a rescan, and changing the level clears them. Keep
candidates are never ticked.

**Caches move in (D-1).**

- `caches::scan()` results become candidates. Category: Package caches (npm,
  pnpm, Yarn, Bun, Cargo, Homebrew), or Xcode (DerivedData whole,
  DeviceSupport, unavailable simulators).
- Tiers: DerivedData whole is Rebuild; DeviceSupport is Rebuild;
  `simctl delete unavailable` is Safe; package caches are Safe; Homebrew
  cleanup is Safe.
- The Folder sizes page loses `caches_section` and gains a one-line link,
  "Caches moved to Cleanup".

**Clean up (FR-12)**

- The dialog title reads "Clean up 52 GB?". The detail lists each ticked
  category with its count and size, and adds these when they apply:
  - "Worktrees are removed with git; their branches stay."
  - "Folders go to the Trash."
  - "Homebrew and simulator cleanup can't be undone."
- The buttons are [Cancel, Clean up]. Then `remove::run` runs on a worker,
  rows lock, and a progress line shows "Cleaning 7 of 40…".
- The result card shows "Freed 49 GB · 12 GB in the Trash · 2 skipped
  (show)", then a rescan follows.

**Scan state and streaming (NFR-1, D-9).**

- `ScanState` holds `categories`, `skipped`, `phase` (Finding | Measuring |
  Done) and `started`.
- The coordinator publishes each repo's worktrees as soon as they are
  classified, then the sizing workers fill `bytes`.
- Size cache: `HashMap<(dev, ino), (mtime, head, bytes)>` kept in memory for
  the app's life. An unchanged folder is not walked again.
- Cancel sets the `AtomicBool`. Workers check it between git calls and inside
  `folder_bytes` every 1,000 entries; add that check to `folder_bytes`.

**Budget (NFR-1)**

| Hop | Budget |
|---|---|
| Find repos in 4 roots, depth 3 | ≤ 300 ms |
| `worktree list`, 11 repos on 4 workers | ≤ 300 ms |
| First repo classified and drawn | ≤ 1 s → first rows < 2 s |
| All 86 worktrees: ~6 calls × ~20 ms each, on 4 workers | ≈ 2.6 s, target ≤ 10 s |
| Sizes, warm: cache hits plus changed folders only | ≤ 60 s |
| Sizes, cold | Measured and recorded; streams, no target |

**Tests**

| AC | Test | Type |
|---|---|---|
| FR-9 | `gate::keep_never_ticked` | unit |
| FR-10 | `levels_tick_expected_tiers`, `manual_tick_survives_rescan`, `level_change_clears_overrides` | unit |
| FR-11 | `category_meta_line_formats` (en, nb) | unit |
| FR-12 | `confirm_detail_lists_categories_and_undo_notes` | unit |
| FR-1, NFR-1 | `scan_streams_first_category_before_sizes` (fixture with slow sizing) and `cancel_stops_within_1s` | integration |
| NFR-1 | Timed run on Henrik's Mac: first rows, all states, warm and cold totals | manual, recorded in section 10 |
| NFR-5 | `locale_guard` passes; light and dark screenshots of Cleanup | unit + manual |
| D-1 | Sidebar order test updated; Folder sizes shows the link | unit + manual |

**Rollout.** Slices 1 and 2 must both be merged before the next release
(slice 1's plain list is not meant for users). Releasing stays Henrik's call.

### Slice 3 — Leftovers: orphan build data, build output, prune, locks

Delivers FR-5, FR-6, FR-7, FR-13 (tool locks).

**Orphan DerivedData (FR-6)**

- For each `~/Library/Developer/Xcode/DerivedData/*/info.plist`, read
  `WorkspacePath` with `/usr/bin/plutil -extract WorkspacePath raw -o - <plist>`
  (2 s timeout).
- The folder is an orphan when the path does not exist, **and** it does not
  start with `/Volumes/<name>` for a volume that is not mounted.
- No plist, or no key, means keep. Orphans are Safe, method Trash. They are
  validated with the same rules as `caches::validate`: inside home, no links,
  canonical.
- When orphans exist, the whole-DerivedData candidate from slice 2 stays
  Rebuild, and the orphans are listed first in Xcode build data.

**Build output (FR-7)**

| Build file | Output folders |
|---|---|
| `package.json` | `node_modules`, `.next`, `.turbo`, `dist` |
| `Cargo.toml` | `target` |
| `Package.swift` | `.build` |
| `build.gradle(.kts)`, `settings.gradle(.kts)` | `build`, `.gradle` |
| `pyproject.toml`, `requirements.txt` | `.venv`, `__pycache__` |

- Projects are folders up to 4 levels below each root that hold a build file.
- Skip any project inside a worktree candidate (its removal covers it), and
  any project with a process inside (Lock: "In use by node").
- The output folder must sit next to the build file. It must be ignored or
  untracked: `git check-ignore -q <dir>` exits 0, or the project is not in a
  repo. It must contain no tracked file: `git ls-files <dir>` is empty.
- Idle time is the last commit time, or for a folder that is not a repo, the
  newest mtime of the build file and its top-level source entries. Output
  folders don't count.
- Tier is Rebuild, method Trash, category "Project build output". The title
  reads "catchlog · node_modules · idle 2 months".

**Prune (FR-5).** Use `git worktree list --porcelain`'s `prunable` records. In
Worktrees they show as "3 stale git records". When ticked, run
`git -C <main> worktree prune`. These are Safe.

**Tool locks (FR-13)**

| Category | Locked while a process runs whose name or args match |
|---|---|
| Xcode build data, DeviceSupport | `Xcode`, `xcodebuild`, `XCBBuildService` |
| Gradle | `java` with `GradleDaemon` in args |
| Simulators | `Simulator`, or a `launchd_sim` child |
| Package caches | the tool itself: `npm`, `pnpm`, `yarn`, `bun`, `cargo`, `brew` |

The lock chip reads "Waits for Xcode". `recheck` repeats the test.

**Tests:** `derived_orphan_when_workspace_missing`,
`derived_kept_when_volume_unmounted`, `derived_kept_without_plist`,
`build_output_only_next_to_build_file`, `build_output_skips_tracked`,
`build_output_idle_from_last_commit`, `prune_counts_and_clears`,
`xcode_lock_blocks_derived` (fake process list). All are fixtures or unit
tests.

### Slice 4 — More places and "Is it really files?"

Delivers FR-8, FR-15.

**Catalog (FR-8).** These are added to `leftovers.rs`. disk-jockey's list
(MIT) is credited in a comment, and in `README.md` under Acknowledgements.

| Category | Path (under `~` unless absolute) | Tier | Method |
|---|---|---|---|
| Package caches | `.gradle/caches`, `.gradle/wrapper/dists` | Rebuild | Trash |
| Package caches | `Library/Caches/CocoaPods` | Safe | Trash |
| Package caches | `Library/Caches/pip`, `Library/Caches/pypoetry`, `.cache/uv` | Safe | Trash |
| Package caches | `go/pkg/mod` | Rebuild | Tool: `go clean -modcache` |
| Package caches | `Library/Caches/org.swift.swiftpm` | Safe | Trash |
| Package caches | `Library/Caches/ms-playwright` | Rebuild | Trash |
| Logs and crash reports | `Library/Logs/DiagnosticReports` files > 30 days, `Library/Logs/CoreSimulator` | Safe | Trash |
| Agent scratch | `/private/tmp/claude-<uid>/*` subfolders untouched for 3 days, owned by you | Safe | Trash |
| Downloads and installers | `Downloads/*.dmg`, `*.pkg`, `*.zip` > 30 days; **only when `cleanup_include_downloads` is on** (default off: avoids the macOS privacy prompt) | Rebuild | Trash |
| Kept | `Library/Developer/Xcode/Archives`, `.claude/projects` (agent history), `Library/Application Support/MobileSync/Backup` | Keep | — |

Agent scratch is the one place outside home. `gate` allows exactly
`/private/tmp/claude-<getuid()>/` and its children, checked with an
owner-uid test.

**Pressure card (FR-15)** (`pressure.rs`, read when the page opens):

| Line | Source | Action |
|---|---|---|
| Swap used | `sysctl vm.swapusage`, or the existing memory reading if it already has swap | Info: "Quitting apps frees swap" plus a link to Apps & processes |
| Local snapshots | `/usr/bin/tmutil listlocalsnapshots /`: count, and names with `com.apple.os.update-` | Info: "A staged macOS update holds deleted space until it installs" |
| Booted simulators | `/usr/bin/xcrun simctl list devices booted -j`; memory from processes under `launchd_sim` | Shut down (asks first): `xcrun simctl shutdown <udid>` |

The card hides itself when every line is zero. A source that fails hides its
line.

**Settings:** `cleanup_include_downloads: bool = false`,
`cleanup_build_idle_days: u32 = 30`.

**Tests:** one fixture test per catalog row (path, tier, method), plus
`scratch_only_own_uid`, `downloads_off_by_default`, `parse_swapusage`,
`parse_tmutil_update_snapshots`, `parse_simctl_booted`.

### Slice 5 — The low-disk alert opens Cleanup

Delivers FR-16.

- `notifications::send(message)` becomes `send(message, metric: &str)`. The
  metric goes into the request identifier,
  `tidegauge-<metric>-<pid>-<time>-<seq>`, so no userInfo is needed.
  `app.rs:202` passes `a.metric`, and `send_test` passes `"test"`.
- `did_receive` reads the identifier from
  `response.notification().request().identifier()`.
  - `low_disk` → `statusbar::raise(&SHOW_CLEANUP)`, a new `AtomicBool`
    beside `SHOW_SETTINGS`.
  - Anything else → `TOGGLE_PANEL`, as today.
- `app.rs` handles `SHOW_CLEANUP` like `SHOW_SETTINGS`: show the window,
  `navigate(Destination::Tool(Page::Cleanup))`, and start a scan.
- The Settings → Alerts row for low disk gains the hint "Clicking it opens
  Cleanup".

**Tests:** `identifier_carries_metric`, `parse_metric_from_identifier`. Manual:
a test alert with metric `low_disk` opens Cleanup (packaged app only, since
notifications need the bundle).

### Slice 6 — Agent review (opt-in)

Delivers FR-18, NFR-4. Decisions D-4 and D-10.

**Settings → Cleanup → "Ask an agent before cleaning"**

- `cleanup_agent: Option<AgentKind>` (None by default), where
  `AgentKind = ClaudeCode | Codex`. The row shows the resolved binary path,
  or "Not installed".
- Lookup folders: `~/.local/bin`, `~/.claude/local`, `/opt/homebrew/bin`,
  `/usr/local/bin`, `~/.bun/bin` and `~/.npm-global/bin`.
- `cleanup_agent_consent: Option<i64>` holds the time you accepted the
  first-run notice: "Paths, sizes, git states and the contents of files the
  agent opens are sent to Anthropic/OpenAI by your agent."

**Page.** A "Ask Claude Code" button sits beside Clean up, and appears only
when an agent is on. It reviews **ticked** candidates only, at most 60.

**Input** (on stdin, inside the prompt):

```json
{ "task": "For each item, say keep or clean. Clean only if it is a disposable leftover of finished work. Answer only with JSON.",
  "items": [
    { "id": "c1", "category": "AgentWorktrees", "path": "/Users/henrik/Dev/Repos/brev/.claude/worktrees/fix-auth-3f2a",
      "state": "Removable", "branch": "claude/fix-auth-3f2a", "last_commit": "2026-10-28", "bytes": 2147483648 } ] }
```

The `id`s are short aliases (`c1`…) that map to candidate IDs inside
Tidegauge, so the agent never echoes a real path back as an instruction.

**Output** (strict):

```json
{ "notes": [ { "id": "c1", "verdict": "clean", "reason": "Branch merged in PR 412; no local changes." } ] }
```

**Commands** (D-10; the working folder is an empty temp folder)

- Claude Code:
  `claude -p --output-format json --allowedTools Read Glob Grep --disallowedTools Bash Edit Write WebFetch WebSearch --add-dir <path>…`,
  with the prompt on stdin. The JSON's `result` field holds the answer.
- Codex: `codex exec --sandbox read-only --skip-git-repo-check -o <tmpfile> -`,
  with the prompt on stdin. The answer is read from `<tmpfile>`.
- The environment is inherited, so your agent login works. Tidegauge adds no
  keys and reads none.

**Parsing and limits**

- Take the first `{…}` block that parses into the schema.
- Drop unknown ids and duplicates, and cut each reason to 140 characters.
- "keep" unticks the row and shows the reason in a note chip. "clean" shows a
  check chip and does not change ticks.
- Keep-tier items are never sent and can never be ticked.
- One review at a time, 180 s limit. Cancel kills the child.

**Failure modes**

| Failure | What happens |
|---|---|
| Timeout | Note "Claude Code did not answer in 3 min"; nothing changes |
| Exit code not 0, or bad JSON | Note "Couldn't read the agent's answer"; nothing changes |
| CLI missing | The button is hidden; Settings says "Not installed" |
| Agent asks for permission or input | It can't: stdin is closed after the prompt and the run times out |
| Prompt injection inside a file name or commit | Notes are advice only and can only untick; removal still needs your confirmation and `recheck` |

**Tests:** `agent_parse_strict_schema`, `agent_drops_unknown_ids`,
`agent_keep_unticks_clean_does_not_tick`, `agent_never_sends_keep_tier`,
`agent_timeout_changes_nothing` (fake CLI script), `agent_off_makes_no_child`.
Manual: one real run each with Claude Code and Codex on fixture worktrees,
recorded in section 10.

## 10. Verification

Filled in as each slice lands. A row without a passing result keeps its slice
open.

| Acceptance criterion | Test | Result | Change |
|---|---|---|---|
| FR-2 | `worktrees::finds_claude_codex_and_dot_worktrees` | pass (en, nb) | this change |
| FR-3 | `classify_rules_in_order`, `classify_against_real_git`, `in_use_when_cwd_inside`, `locked_and_orphan`, `disposable_names`, `ignored_dev_state_keeps_a_merged_worktree` | pass | this change |
| FR-4 | `remove::remove_keeps_branch_and_drops_listing`, `git_refuses_dirty_even_if_recheck_passed` | pass | this change |
| FR-5 | `worktrees::prunable_records_counted` | pass | this change |
| FR-6 | `leftovers::derived_orphan_when_workspace_missing`, `project_gone_respects_unmounted_volumes` | pass | this change |
| FR-7 | `leftovers::build_output_rules`, `build_output_skips_worktrees` | pass | this change |
| FR-8 | `leftovers::catalog_places_and_tiers` | pass | this change |
| FR-9, FR-10 | `cleanup_panel::levels_tick_expected_tiers`, `manual_tick_wins_and_keep_never_ticks` | pass | this change |
| FR-11, FR-1 | `cleanup_panel::summary_and_meta_lines`; rendered check with a fake home (light and dark) | pass: header, levels, categories, Kept group, Clean up button; no overlaps | this change |
| FR-12 | Confirmation text built in `clean_up`; removal via `remove::run` tests | unit pass; dialog not exercised by a test | this change |
| FR-13 | `in_use_when_cwd_inside`, `recheck_refuses_after_folder_swap_and_new_process`, `gate::package_cache_waits_only_for_its_own_tool` | pass | this change |
| FR-14 | `gate::protected_paths_never_listed`, `parent_of_protected_refused`, `validate_refuses_links_and_outside_home` | pass | this change |
| FR-15 | `pressure::parses_tmutil_update_snapshots`, `parses_simctl_booted` | pass | this change |
| FR-16 | `notifications::identifier_carries_metric` | pass; click routing needs a packaged build (not run) | this change |
| FR-18 | `agent::*` (4 tests), `cleanup_panel::agent_keep_unticks_clean_does_not_tick` | pass; one real run per agent not done | this change |
| NFR-1 | Timed run on Henrik's Mac | Cold `cleanup worktrees` in a debug build: 5 min 30 s, almost all spent sizing folders. Over the 60 s target, which is for a warm rescan (not timed) | this change |
| NFR-2 | `recheck_refuses_after_new_commit_or_file`, `recheck_refuses_after_folder_swap_and_new_process`, `leftovers::build_output_rules` (tracked after scan) | pass | this change |
| FR-10, FR-12 | `cleanup_panel::nested_items_count_once` | pass | this change |
| NFR-3 | Every call goes through `packages::run_command` or `bounded::Probe`; the shared timeout tests in those modules | pass (existing tests) | this change |
| NFR-4 | Code path: `gh` and agent run only behind settings that default off | reviewed, no test | this change |
| FR-17 | Menu panel reads `cleanup::READY` (set at the end of a full scan) | build only; not clicked | this change |
| FR-19 | ⌘⌫ handler on the dashboard calls `clean_up` on Cleanup | build only; not pressed | this change |
| FR-20 | `palette::tests::filters_by_every_word` | pass; bar not opened in a rendered check yet | this change |
| FR-21 | `leftovers::app_leftovers_only_for_gone_vendors` | pass | this change |
| FR-22 | `remove::branch_delete_only_when_merged` | pass | this change |
| FR-23, FR-24 | `auto::auto_takes_only_safe_undoable_items` | pass; the 30-minute loop not run end to end | this change |
| FR-25 | `cli::mcp_lists_tools_and_rejects_unknown`, `cli_ignores_app_arguments`; read-only run on Henrik's Mac: `cleanup worktrees --merged` listed 85 Removable worktrees (25 GB), two checked by hand as merged, clean and pushed; `hook session-start` silent above the limit; `mcp` answered `initialize` and `tools/list` | pass | this change |
| FR-26 | `agent::every_agent_runs_read_only` (all seven, no auto-approve flag) | pass; no real run of any agent | this change |
| FR-27 | `leftovers::new_catalog_entries_follow_their_tiers` | pass | this change |
| FR-28 | `leftovers::xcodebuildmcp_workspaces_only_when_idle` | pass | this change |
| FR-29 | `pressure::knows_emulators_and_daemons` | pass; card not checked on screen | this change |
| FR-30 | `hooks::denies_at_cap_and_lists_devices`, `allows_already_booted_and_under_cap`, `emulator_cap_and_pressure`, `unrelated_bash_is_no_boot`, `malformed_input_prints_nothing` | pass; not run inside a real agent | this change |
| FR-31 | `hooks::session_add_remove_and_stale`, `last_session_shuts_down_only_new_simulators`, `session_file_roundtrip_under_lock` | pass | this change |
| FR-32 | `hooks::emulator_cap_and_pressure` | pass | this change |
| FR-33, FR-34 | `remove::dirty_worktree_can_be_trashed_by_hand_only_when_safe` | pass; the Trash move itself is not run in tests (it would use the real Trash) | this change |
| FR-35 | `auto::ladder_takes_old_build_output_oldest_first_until_enough` | pass | this change |
| FR-36 | `uninstall::finds_only_exact_leftovers`, `only_apps_in_applications_and_never_apple_or_tidegauge` | pass; no real uninstall run | this change |
| FR-37 | `gate::names_who_started_a_helper` | pass | this change |
| FR-38 | `cleanup_panel::level_hint_says_what_it_picks_and_warns_about_resets`; layout checked on screen | pass | this change |
| FR-39 | `cleanup_panel::each_item_shows_where_it_is_in_a_run`, `remove::progress_records_each_item` | pass; not yet seen on screen during a run | this change |
| NFR-5 | Full suite with `TIDEGAUGE_TEST_LANG=en` and `=nb`, including `locale_guard` | 511 passed, 0 failed each | this change |

## 11. Codebase deltas and change log

### Codebase deltas

| Where code and design differ | Ruling |
|---|---|
| No checkbox widget exists; only `switch` and selectable rows | Code migrates: add `controls::checkbox` in slice 2 |
| `capacity_bar` and `usage_bar` are private, single-colour | Code migrates: a stacked bar (used, ready, free) in slice 2 |
| A notification click always opens the menu-bar panel and ignores userInfo | Code migrates in slice 5 |
| Tidegauge runs no `git` today | New, bounded, absolute path (CON-3) |
| README says Tidegauge never calls a provider | Text gains the opt-in exception with slice 6 |
| `bounded::Probe` allows one child at a time; the git workers need four | Code migrates: use `packages::run_command`, made `pub(crate)` if needed (slice 1) |
| `caches::folder_bytes` cannot be cancelled | Code migrates: add a cancel flag, checked every 1,000 entries (slice 2) |
| `notifications::send(message)` carries no metric | Code migrates: `send(message, metric)` (slice 5) |
| Design put `checkbox` in `controls.rs` and `stacked_bar` in `ui.rs` | Built inside `cleanup_panel.rs`, their only user; move them if a second page needs them |
| Design named ⌘⌫ for Clean up | Not built: the page has no focus target for key events yet. Later |
| Design named an `#tool:` suffix for tool ids | Built as `tool:<Kind>`, `sim:<udid>` and `prune:<repo>` |
| Classification rule 7 and 8 | Changed by D-11 |
| `leftovers::build_output` read the home folder itself | Takes `home` as a parameter so fixtures can run outside the real home |
| `caches.rs` kept freed-size helpers and `open_trash` for the old Storage section | Removed with that section; Cleanup measures freed space from free-disk change |
| Build-output names that make an ignored file disposable (section 9) | Narrowed after review to the design's list plus caches and editor state, matched on the whole path or top-level entry only. Dev-server state such as `.wrangler/` makes a worktree Review |
| `git worktree remove` ran under the 10 s read timeout | Writes get their own 10-minute limit (`git::WRITE_TIMEOUT`) so git is never killed halfway |
| Agent scratch was Safe | Rebuild: Balanced takes it after 30 idle days, since a live session can work below the top level |
| Old `.zip` files in Downloads were listed | Only `.dmg` and `.pkg`: a zip may be your own archive |
| `dist` and `build` outside any repo were build output | Skipped: without git nothing proves they aren't source |
| App leftovers trusted any list of installed apps | Fails closed: no list (unreadable /Applications, a failed id read) means no leftovers; Toolbox and Setapp folders and deeper nesting are searched |
| A path listed twice under two names (SwiftPM cache) | Listed once; identical paths count once in a plan |
| CLI and MCP removal had no switch | "Let agents remove merged worktrees", off by default |
| `go clean -modcache` used Go's own setting | `GOMODCACHE` is pinned to the `~/go/pkg/mod` the row shows |
| A package cache waited for any package tool | Waits only for its own tool (`gate::cache_tools`), found in the rendered check |
| Recheck of build output compared only path and inode | It also checks again that git ignores and does not track the folder (`leftovers::still_output`) |
| A ticked orphan inside a ticked whole DerivedData was counted and removed twice | The plan drops any item that sits inside another ticked item; known caches carry a last-used time, so Balanced leaves a DerivedData in use alone |
| Stop did not interrupt measuring the known caches | `caches::scan_until` passes the cancel flag to every folder walk |
| Agent CLI flags (D-10) | Checked 2026-11-09 against the installed `claude` and `codex`: Claude Code uses `--tools Read Glob Grep` plus `--no-session-persistence`; Codex adds `--ephemeral` |

### Change log
- 2026-11-09: first draft from Levo, disk-jockey and the 34a6bc2 code.
- 2026-11-09: detailed design written for all six slices on the draft answers,
  so they can be built in order. Added D-8 to D-10, the verification table,
  three deltas, and a sharper NFR-1 (cold sizing streams; the warm rescan has
  the 60 s target).
- 2026-11-09: all six slices built in one change on `feat/cleanup-page`.
  Added D-11, the deltas above, and the verification results.
- 2026-11-09: review fixes: D-12 (orphans kept), narrower disposable list,
  long timeout for git writes, scratch and Downloads tightened, per-tool
  cache locks.
- 2026-11-09: slices 7–11 added and built after Henrik chose "everything"
  (D-13 to D-16, FR-19 to FR-26).
- 2026-11-09: PR review fixes: build-output recheck, no double counting of
  nested items, cancellable known-cache scan.
- 2026-11-10: slices 12–16 added after Henrik asked for every missing disk-jockey
  and Levo feature plus a worktree analyzer; Q-2 reversed by D-18.
- 2026-11-10: slices 12–16 built. Sandboxed containers stay out of the
  uninstall list (reading them prompts for privacy access); emulators are
  counted and capped but never shut down by the hook.
- 2026-11-10: slice 17 after Henrik found "In use by sleep" and the
  checkbox and expand arrow unclear.
- 2026-11-10: slice 18 after Henrik found the running cleanup hard to
  follow (only "Cleaning 69 of 293…" in the footer).
