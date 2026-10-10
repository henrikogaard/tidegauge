# Tidegauge historical UI/UX findings

These recovered findings describe an earlier build and environment. They are not validation of the current checkout; some issues have since changed.
Build: `target/debug/tidegauge` @ commit 34e4223. Env: macOS 26.5.2 arm64 VM (EC2 Mac).

## Top navigation (the change) — PASS
- Left sidebar fully replaced: logo ("P" tile + "tidegauge") left, all 11 tabs in one horizontal row: Overview, CPU, Memory, Disk, Network, GPU, Neural, Battery, Sound, Bluetooth, Projects. Icon-over-label style with per-metric accent colors matching the theme.rs palette.
- Active tab highlighted (card_hi pill + semibold label); inactive tabs dimmed.
- Footer strip bottom-right: "516 apps · 958 processes" (live counts).
- Tab labels are ~8px — small but legible; active label's semibold change repacks neighbors ±a few px (cosmetic).

## Tab-by-tab (click-through, all verified visually)
| Tab | Result | Notes |
|---|---|---|
| Overview | PASS (with bug B1) | 6 mini-tiles (CPU%, Mem%, GPU —, Disk%, Net rate, Batt —) + 3 donut cards render with live values |
| CPU | PASS (with bug B1) | Hero "CPU USAGE" 15–21%, "Apple M4 Pro (Virtual) · 12 P + 0 E cores", load/uptime footers, 4 stat cards, APP|CPU ranked rows w/ rank#, tiles, "N processes", bars, % |
| Memory | PASS (with B1) | Hero 51%, "8.16 GB used of 16.0 GB", App/Wired/Compressed/Swap cards (Swap "None"), app rows in MB/GB |
| Disk | PASS (with B1) | Hero 0 B/s, "92.4 GB used of 123 GB · 2 volumes", Read/Write/Free/Volumes cards, app rows |
| Network | PASS (with B1) | Hero ~200–500 KB/s, "Wi-Fi · en0", Down/Upload/Peak footers, Total in/out cards, app rows |
| GPU | PASS — different degradation than expected | VM reports a paravirtualized GPU: "Apple GPU", Supported: **Yes**, GPU memory 747 MB, utilization "—". The `!supported` "Not available on this Mac" path does NOT trigger on this host. Renders cleanly, no crash. |
| Neural | PASS — same caveat | VM reports ANE: "Apple Neural Engine · 16 cores", State "Ready", util "—". The "Not available" path not exercised here. |
| Battery | PASS | "No battery" status, "—" metrics, System power ~1.7 W, APP|POWER USAGE list (30 rows), bottom explainer: "This Mac has no battery — showing per-app power draw instead." |
| Sound | PASS (note N4) | Output card "Default output" 0%, volume bar (display-only), device chip, APP|VOLUME list of audio daemons "Idle" + level meters. No interactive control exists in code — volume display only. |
| Bluetooth | PASS | "No devices connected", "Bluetooth Settings" link, "Paired devices will appear here…" (bt.enabled=true on VM). The "off or unavailable" branch not exercised but code path exists. |
| Projects | PASS | "1 dev servers are running but idle" (N3: singular grammar), "Stop All…" + per-row "Stop" buttons present — NOT clicked (they call kill_pid directly, no confirmation despite the "…"). One server detected: "code :6789, idle 7 minutes". |

## Interactions — PASS
- Scroll reset on tab switch: scrolled CPU app list, switched to Memory and back → CPU view back at top (fresh ScrollHandle per switch).
- Tab switching is instant; content swaps correctly every time.
- Letter-tile fallback works for daemons (H, B, N, I, A tiles seen).

## Menu bar — PASS (with minor notes)
- 3 NSStatusItems visible top-right: network "521 KB/s" + sparkline, memory "51%", cpu "15%" + sparkline — live values matching the panel.
- Click raises the panel (verified). N5: click only *activates* — it cannot hide the panel ("toggle" is one-directional); no menu/Quit exists (right-click also just toggles; `_quit` selector is dead code).
- N6: `MenuItem.style` ("value"|"graph"|"both") is ignored — every item renders a sparkline.

## BUGS
- **B1 (major, pre-existing — canvas render offset):** all `canvas()` charts paint displaced to the content area's top-left instead of inside their cards. Evidence: stray ~110px gray circle overlapping the Overview top-left (the POWER BY APP donut), a chart fragment band below the tab bar, all 6 mini-tile interiors blank, all 3 donut cards ringless (legend+text only). On metric tabs the hero area chart floods over the hero label/value and bleeds past the card's left edge. `area_chart`/`donut` paint paths with local coords; apparently the canvas paint space isn't being translated to the element bounds.
- **B2 (minor):** blank app-icon slots — some system apps show a 20px blank space; `icon_key_for_bundle` presumably returned a key but the PNG renders empty/invisible, and there is no fallback to the letter tile in that case (fallback only when no key).
- **B3 (cosmetic):** "1 dev servers are running but idle" — singular/plural not handled.
- **B4 (minor):** CPU hero footer "Uptime" shows app lifetime (`self.started.elapsed()`), not system uptime (system is 1h01m, app showed "9 minutes"). Misleading label for a system monitor.
- **N4:** Sound "volume controls" are a display-only bar; no slider/mute interaction exists (per code, no click handlers).
- **N5/N6:** see menu-bar notes above.

## Evidence
The earlier environment included a recording and screenshots. Those artifacts are not included in this repository.
