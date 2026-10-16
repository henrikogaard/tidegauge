# Tidegauge design reference

Dense, Apple-native data UI. The panel follows the macOS appearance; menu-bar readouts query the native appearance on each refresh. See `src/theme.rs` for implemented tokens.

## Palette

Graphite Night uses a charcoal canvas, mineral surfaces, silver text, and neutral controls. Graphite Day uses the matching light palette. All app colors come from `src/theme.rs`; the panel follows macOS appearance.

| Token | Graphite Day | Graphite Night |
|---|---|---|
| `bg` | `#f5f5f7` | `#161618` |
| `card` | `#ffffff` | `#1e1e20` |
| `card-hi` | `#ededf0` | `#2c2c2e` |
| `ink` | `#1d1d1f` | `#e5e5ea` |
| `ink-dim` | `#6e6e73` | `#8e8e93` |
| `line` | `#d2d2d7` | `#38383a` |
| `accent` | `#1d1d1f` | `#f5f5f7` |
| Success | `#248a3d` | `#30d158` |
| Error or critical state | `#d70015` | `#ff453a` |

## Metric colors

Surfaces, body text, and controls stay neutral in both appearances. Charts, metric headings, and selected navigation use consistent metric colors. Labels, units, and values remain readable without relying on color. App breakdowns use progressively softer shades with a labeled legend.

| Metric | Graphite Day | Graphite Night |
|---|---|---|
| CPU | `#246fc7` | `#5a9cf0` |
| Memory | `#6550bb` | `#a397f4` |
| GPU | `#b94378` | `#f18db5` |
| Disk | `#9b6900` | `#e8ad39` |
| Network | `#147d59` | `#41c79a` |
| Battery, Power | `#287c35` | `#63ca6e` |
| Sound | `#ab338c` | `#e87bcc` |
| Bluetooth | `#087d99` | `#51c4df` |
| Projects | `#ba4f22` | `#f28d60` |
| Neural | `#8648b8` | `#c28df2` |

The soft metric tone uses 16% alpha. Chart fills use lower opacity. Selected navigation uses a 12% metric tint without an outline; keyboard focus uses a visible metric-colored border.

## Layout

- **Window**: 1192×780 with a 1050×650 minimum, resizable native window with a transparent titlebar and system window controls. Closing hides the dashboard; a menu-bar readout reopens it.
- **Navigation**: a 224 px sidebar grouped as Live, Work, Over time and Maintenance, with Settings pinned at the bottom. Live entries carry a metric swatch and a short reading; the selected entry gets a quiet metric tint. Hardware a Mac doesn't have (no battery, as confirmed by macOS) has no entry. Work holds Apps & processes and Agents (development servers show inside Agents); Over time holds History and Before & after (recent activity is the "Last minute" view of Apps & processes). Tab/Shift-Tab moves keyboard focus; Enter/Space opens an entry or metric card.
- **Page header**: the page title (metric pages in their metric colour), at most one subtitle line saying what the page is for or counting what it shows, the page's own actions on the right, and an ⓘ button. ⓘ opens "About this page" at the top of the content: what the numbers mean and where they stop. Caveats live there, never as grey paragraphs on the page, and every page opens with them closed.
- **Cards**: 12 px radius, `card` fill, 1 px `line` border.
- **Overview**: six 208 px metric cards in three columns (154 px in the user-toggleable `compact_dashboard` mode), 12 px radius, 16 px gaps (8 px in compact mode); 34 px headline with a separate unit, three supporting readings, and a 38 px tinted strip holding a filled sparkline. Clicking a card opens its detail tab.
- **Hero card**: caption, 42 px number, sub-caption, filled area chart (130 px), footer stat triplets.
- **App rows**: icon, name + `N processes`, usage bar, right-aligned value.
- **Icons**: an app's own icon when its bundle names one. Otherwise a 20 px soft tile with a line glyph: a window for an app without an icon, a terminal for a background program, a bot for an agent session, a package, beer or hexagon for Homebrew casks, formulae and npm packages. Never a letter tile or macOS's grey placeholder.
- **List rows**: Apps & packages, Startup and Recent activity share one row: icon, name, one quiet second line, values right-aligned in fixed columns. A row that can be picked gets a hover, a selected tint and keyboard activation, and shows its details and actions underneath while selected. Long lists show 60 rows and a "Show more (N)" button, never pages.
- **Donuts**: convex ring segments (inner radius 0.70), 92 px chart with a center value and subtitle, legend beside the chart with actual amounts and app icons. App breakdowns show the top four entries and Other. Donut cards pad 16 px with 12 px gaps.
- **Fonts**: `.AppleSystemUIFont`; captions uppercase 10 px semibold, body 12–14 px, page titles 20 px, hero number 42 px bold (sizes are the Tokens below). Explanatory sentences and honesty disclaimers stay at 12 px.
- **Spacing**: 4/8/12/16/20/24 grid; 20 px page gutter; lists always expanded.

Overview sparklines share the detail-tab area chart and show the same recent samples. CPU/GPU averages and peaks cover the current session; network totals are interface counters since boot. A card without a reading says "Not available on this Mac" in place of the number and keeps only the supporting values that exist. The card grid never leaves a hole: a short last row takes a "Busiest apps" card (top apps by CPU, opens Apps & processes), which otherwise sits beside the memory donuts. Per-app power remains labeled as estimated.

## Tokens

Typography, corner radii and layout spacing come from named constants in `src/theme.rs`; raw `px(..)` literals for those are rejected by a unit test. Chart geometry, icon sizes and column widths stay literal.

| Group | Tokens |
|---|---|
| Text | `TEXT_CAPTION` 10, `TEXT_SMALL` 11, `TEXT_BODY` 12, `TEXT_ROW` 13, `TEXT_EMPH` 14, `TEXT_TITLE` 20, `TEXT_STAT` 34, `TEXT_HERO` 42 |
| Radius | `RADIUS_BAR` 2, `RADIUS_CHIP` 6 (badges, icon tiles, row-highlight bars), `RADIUS_CONTROL` 8 (controls, list rows, toasts, notices), `RADIUS_CARD` 12 |
| Space | `SPACE_1` 4, `SPACE_2` 8, `SPACE_3` 12, `SPACE_4` 16, `SPACE_5` 20 (page gutter), `SPACE_6` 24; `SECTION_GAP` = `SPACE_4` between detail-page sections |
| Height | `CONTROL_H` 28, `ROW_H` 36 |

## Detail-page template

Every metric tab composes the same ordered parts with 16 px between sections:

1. **Hero card** — uppercase 10 px caption in the metric accent; 42 px bold value with the unit as separate smaller text; one sub-caption line for hardware/context; filled area chart (130 px) under a single axis line: how far back it reaches ("−8 min"), the high-water legend, and "now"; a footer row of stat triplets.
2. **Attribution section** — a standard section header (uppercase 10 px `ink-dim` label left, optional qualifier right) followed by app rows in the anatomy above. Present only where genuine per-app data exists.
3. **Supplementary sections** — same section-header pattern, each in a 12 px-radius card: CPU → temperatures and fans (hottest and average first, the native channel list folded behind a button); Disk → devices and volumes; Battery → battery health and electrical details; Sound → output control; Bluetooth → devices. The Neural Engine has no section until a real reading exists.

Page expectations beyond the template: History opens with a controls card that clearly separates Range, Source, and Metric; loading, empty, and error outcomes use the shared empty state rather than an empty plot; it reloads every minute while open (no Refresh button), and when recording began inside the range the plot says "Recorded since 14:10" instead of leaving the empty left side unexplained; its ⓘ notes state the 30-day hourly-pruned retention, means-not-peaks, missing-not-zero, the top-60 recorded app-name bound, multi-core CPU, and estimate scope. Disk's hero reports system-wide read+write activity, not capacity; physical device capacity lives inside the device identity card, and volumes share one APFS container header only when the container's total/free/stores metadata is consistent — shared space is shown once and never summed per volume. volumes in a container share one capacity bar. Development servers (Projects) show as a section of Agents with a count summary card that counts distinct server processes rather than listening ports, a CPU-activity idle note, a sectioned list with guarded Stop actions, and a shared empty state when no development servers are detected. Each Tools page says what it is for in its header subtitle and puts its actions in the header; its caveats sit behind ⓘ. Before & after is three numbered steps in one card (measure before, make your change, compare), each with a check when done, and its result is a table of before, after and change, coloured calmer (green) or busier (amber) past ±5 %. Folder sizes offers quick picks (Home, Downloads, Documents, Applications, Caches) and lists folders and largest files with a size bar against the largest. Recent activity renders list rows (process with its path summary, PID, CPU, Disk I/O, Wakeups/s) under a header row. Startup groups registrations by the app bundle their program lives in, else by the maker in their label; Apple's own registrations are hidden behind a switch, a filter field searches app, label and program, and every entry stays reachable through "Show more". Apps & packages lists packages with an available update first. Skipped items on either page are one grey line that opens to show why.

## Empty states

An empty or unavailable surface is a card of the standard radius containing: the metric icon at 20 px on a 16% metric-tint tile; one headline line at 13 px `ink`; at most two lines of explanation at 12 px `ink-dim`. It states plainly what is unavailable and why, and never implies a measurement exists. No wall of prose; no paragraph longer than two lines.

## Controls

All controls share one anatomy: 28 px tall (at least 24 px), a readable 12 px `ink` label, 8 px radius on framed surfaces, transparent fill at rest, `card_hi` on hover, and an accent-coloured focus border. Enter or Space activates the control and stops propagation.

- **Ranked rows** — the usage fill behind a row is drawn only from 1% of the busiest row; smaller shares draw nothing rather than a sliver. Apps & processes holds its row order while the pointer is over the list or an app is open, and shows an "Order held" chip by the sort control whenever the rows on screen differ from the live order.
- **Action buttons** — framed controls: 1 px `line` border, `Button` role; a selected action shows the 12% accent tint. The neutral `accent` applies in Settings and Tools; History, Sound, and Projects actions use the metric accent.
- **Segmented controls** — mutually exclusive choices inside one bordered 8 px surface (`RadioGroup` with an `aria-label` and tab-group semantics) holding `RadioButton` segments with `aria_toggled`, zero gap, and no separate segment outlines or internal separators; the selected segment shows the 12% accent tint, and a reserved transparent border turns full accent on keyboard focus without layout shift. Used for appearance, menu readout styles, Keep awake duration, History range and metric, activity sort, baseline windows, package source, and process filters. Package sort is a single toggle button beside them. Refresh, save, and other actions stay as separate buttons outside any radio group.

- **Switches** — every on/off choice (alerts, Keep awake, launch at login, the menu-bar icon, Overview favorites, compact layout, sidebar visibility, Apple's startup items): a 34×20 track, white knob, system green when on, `Switch` role with `aria_toggled`, Enter or Space flips it.
- **Sliders** — continuous values (alert thresholds 10–100 % in steps of 5, output volume) use the kit slider in the metric colour with the value beside it.
- **Destructive actions** — Force Quit sits beside Quit as an unframed action in the warning colour.

Settings is a grouped list like macOS System Settings: a caption per group (General with Appearance, Language, Launch at login and Keep awake; Alerts; Menu bar; Overview; Sidebar order) over one card whose rows hold a name, at most one quiet line, and one control on the right. Tab-order rows show icon-only up and down buttons in a fixed slot, omitting the up button on the first row and the down button on the last. Icon-only controls carry context-rich localized ARIA labels such as "Move CPU up".

The footer uses one control style for all three actions (Settings, Processes, Tools): a 14 px icon and 12 px label on a 28 px-tall button with 8 px radius, a 1 px `line` border, transparent fill at rest, `card_hi` on hover, and the metric accent on keyboard focus. Settings sits left-aligned; the live app/process counts stay centred as quiet `ink_dim` metadata (not a control); Processes and Tools form a tight right-aligned group with an 8 px gap, flush to the content inset. Keyboard focus order follows visual order after the tabs.

## Menu bar

- Exactly one native `NSStatusItem` stays registered. Its default content is a 16 px Tidegauge wave icon. A left click opens the menu-bar panel under the item; a right or Control click opens the native menu (Open Dashboard, Check for Updates, Settings, Quit), built when shown so it is always in the current language. The update item is disabled in preview builds and live in production.
- **Menu-bar panel**: a 340 px popup under the item with four live tiles (CPU, Memory, Network, Disk: value, one detail and a 26 px curve), up to three AI plans as rows with a ring, and the four busiest apps; a footer opens Tidegauge, Settings or quits. A tile or app opens the full window on that page. Escape, a click in another app, or a second click on the item closes it.
- Readings are opt-in through Settings, with `icon`, `value`, `graph`, and `both` styles. Existing preferences are preserved; Reset to icon only clears optional readings.
- Enabled metrics share one transparent template image. Labels use 9 px system medium and are font-measured once per label (22 px minimum, rounded up) so longer localized labels widen the slot once instead of resizing per sample; values use 11 px system medium with monospaced digits inside fixed-width slots. Spacing between readings is 10 px. Native template tint follows the menu bar and selected state.
- Value and graph readings include CPU, MEM (MINNE in Bokmål), GPU, DSK (DISK), NET (NETT), BAT (BATT), or PWR (EFF) labels. Sparklines are 24×10 px with a 1 px stroke and at most 60 real samples. Missing samples show a dash.
- The icon remains visible when all readings are off, even if its preference is Hidden. The native item is removed when the monitor exits.
- Every dashboard tab has a labeled, keyboard-accessible Settings button in the footer. Command-comma also opens Settings; global Command-Q quits from the dashboard. A per-user file lock prevents duplicate monitors. Settings provides keyboard-accessible choices and reports persistence errors.

## Artwork and documentation preview

The generated activity-bar app icon uses a graphite tile with silver and off-white bars. See [artwork](assets/README.md) for the source PNG, macOS icon package, and generation prompt. UI chrome uses flat colors without gradients.

The README dashboard screenshot is a native render with generic sample data. It demonstrates layout and styling rather than measured hardware activity.
