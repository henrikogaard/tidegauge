# Tidegauge historical UI/UX test plan

This recovered plan describes an earlier build and environment. It is not validation of the current checkout. Use CONTRIBUTING.md for current verification guidance.

App: ~/Dev/Repos/tidegauge Rust+GPUI menu-bar monitor, debug binary already running (PID from `pgrep -fl target/debug/tidegauge`), panel ~860×640 floating NSPanel.

Change under test: commit 34e4223 — left sidebar replaced by top tab bar; app/process count moved to bottom footer.

## Test 1: Top navigation bar layout (primary)
Setup annotation: panel already open showing Overview.
- PASS: horizontal top bar below window top edge: blue "P" tile + "tidegauge" wordmark at LEFT; then all 11 tabs in one row left→right: Overview, CPU, Memory, Disk, Network, GPU, Neural, Battery, Sound, Bluetooth, Projects; each tab = icon over tiny label; labels legible, no truncation/overlap; NO vertical sidebar anywhere; Overview tab visually highlighted (raised card bg).
- FAIL if: any tab missing/overflowing, sidebar still present, logo not leftmost.
- PASS: footer strip at bottom shows "N apps · N processes" right-aligned.

## Test 2: Click through all 11 tabs (primary)
Click each tab left→right; after each click: screenshot + assert content.
- Each click must (a) move highlight to clicked tab, (b) swap content to that tab's view.
- Per-tab expected content:
  - Overview: 6 mini-tiles (CPU %, Memory %, GPU "—", Disk %, Network rate, Battery "—") + 3 donut cards (Memory by Type, Memory by App, Power by App).
  - CPU: hero "CPU USAGE" big % + chip sublabel + area chart + footer stats; 4 stat cards (Load 1m/5m/15m, Cores); "APP | CPU" ranked rows with rank#, app icon or letter tile, name, "N processes", bar, %.
  - Memory: hero "MEMORY" % + "x used of y"; stat cards App/Wired/Compressed/Swap; APP|MEMORY rows.
  - Disk: hero "DISK ACTIVITY" rate + "x used of y · N volumes"; stat cards Read/Write/Free/Volumes; APP|DISK ACTIVITY rows.
  - Network: hero "NETWORK" rate + interface sublabel; stat cards Download/Upload/Total in/Total out; APP|NETWORK rows.
  - GPU: hero "GPU" value "—" + sublabel "Not available on this Mac" + footer Supported "No" + explainer card mentioning virtual machines. NO crash, NO blank.
  - Neural: hero "NEURAL ENGINE" "—" + "Not available on this Mac" + State "Unavailable" + explainer card.
  - Battery: hero "—" + "No battery" status; stat cards; APP|POWER USAGE rows; card "This Mac has no battery — showing per-app power draw instead."
  - Sound: Output card with device name + volume bar + device chips; APP|VOLUME list or "No apps are using audio right now."
  - Bluetooth: "No devices connected" + "Bluetooth Settings"; card "Bluetooth is off or unavailable on this Mac." (no BT hardware on VM).
  - Projects: header "N dev servers running" + "Stop All…" button + per-server rows with Stop buttons; OR if none: "No dev servers detected…" card. Do NOT click Stop/Stop All (real kill_pid, no confirmation).
- FAIL if: tab doesn't switch, wrong/empty content, crash, overlapping/garbled text.

## Test 3: Scroll reset on tab switch (primary interaction)
On CPU tab scroll content down ~3 wheel clicks (app list visible mid-scroll). Click Memory tab, click CPU tab again.
- PASS: CPU view back at scroll-top (hero card visible at top, not app list).
- FAIL if scroll position retained.

## Test 4: Menu bar status items (primary)
Move cursor to top screen edge to reveal macOS menu bar.
- PASS: 3 tidegauge status items top-right showing cpu %, memory %, network rate (defaults from settings.rs); cpu shows value (+sparkline if rendered). Values plausibly match panel (within a few % / same order).
- FAIL if items absent or stale "—"/empty.

## Test 5: Chart/donut rendering (suspected bug — verify)
On Overview: mini-tile chart areas and 3 donut cards.
- Expected (spec): area charts inside each tile; donut rings inside each card.
- Suspected actual (from pre-check): charts/donuts displaced to content top-left (stray gray circle + horizontal band below topbar); tile/card interiors blank.
- Mark PASS only if charts paint inside their own cards. Also check CPU/Network hero charts — same canvas code path.

## Evidence
- Screenshot every tab (11) + menu bar zoom + scroll-reset before/after.
- Findings file: `tidegauge-findings.md`
- One continuous recording of the click-through.
