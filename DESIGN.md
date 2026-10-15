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
- **Navigation**: horizontal top tab bar with all 11 tabs including Overview, metric icon + label, selected tab = quiet metric-tinted surface. Tab/Shift-Tab moves keyboard focus; Enter/Space opens a tab or metric card.
- **Footer**: Settings icon and label, live app and process counts, and the Prosesser and Verktøy entry buttons, on the same uninterrupted canvas. No divider or decorative app-logo tile.
- **Cards**: 12 px radius, `card` fill, 1 px `line` border.
- **Overview**: six 208 px metric cards in three columns (154 px in the user-toggleable `compact_dashboard` mode), 12 px radius, 16 px gaps (8 px in compact mode); 34 px headline with a separate unit, three supporting readings, and a 38 px tinted strip holding a filled sparkline. Clicking a card opens its detail tab.
- **Hero card**: caption, 42 px number, sub-caption, filled area chart (130 px), footer stat triplets.
- **App rows**: rank, icon (rounded tile or cached app icon), name + `N processes`, usage bar, right-aligned value.
- **Donuts**: convex ring segments (inner radius 0.70), 92 px chart with a center value and subtitle, legend beside the chart with actual amounts and app icons. App breakdowns show the top four entries and Other. Donut cards pad 16 px with 12 px gaps.
- **Fonts**: `.AppleSystemUIFont`; captions uppercase 10 px semibold, body 12–14 px, page titles 20 px, hero number 42 px bold (sizes are the Tokens below). Explanatory sentences and honesty disclaimers stay at 12 px.
- **Spacing**: 4/8/12/16/20/24 grid; 20 px page gutter; lists always expanded.

Overview sparklines share the detail-tab area chart and show the same recent samples. CPU/GPU averages and peaks cover the current session; network totals are interface counters since boot. Unavailable hardware readings show a dash. Per-app power remains labeled as estimated.

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

1. **Hero card** — uppercase 10 px caption in the metric accent; 42 px bold value with the unit as separate smaller text; one sub-caption line for hardware/context; filled area chart (130 px) with its scale label top-right and older/now labels at the ends; a footer row of stat triplets.
2. **Attribution section** — a standard section header (uppercase 10 px `ink-dim` label left, optional qualifier right) followed by app rows in the anatomy above. Present only where genuine per-app data exists.
3. **Supplementary sections** — same section-header pattern, each in a 12 px-radius card: CPU → temperatures and fans; Disk → devices and volumes; Battery → battery health and electrical details; GPU → Neural Engine; Sound → output control; Bluetooth → devices.

Page expectations beyond the template: History opens with a controls card that clearly separates Range, Source, and Metric; loading, empty, and error outcomes use the shared empty state rather than an empty plot; a Recording details card holds labeled rows stating the 30-day hourly-pruned retention, means-not-peaks, missing-not-zero, manual refresh, the top-60 recorded app-name bound, multi-core CPU, omitted legacy placeholders, and estimate scope. Disk's hero reports system-wide read+write activity, not capacity; physical device capacity lives inside the device identity card, and volumes share one APFS container header only when the container's total/free/stores metadata is consistent — shared space is shown once and never summed per volume. Projects shows a count summary card that counts distinct server processes rather than listening ports, a CPU-activity idle note, a sectioned list with guarded Stop actions, and a shared empty state when no development servers are detected. Each Tools page opens with one sentence saying what it is for; its caveats sit in a collapsed "About this data" disclosure at the bottom of the page. Recent activity renders as an aligned table (process with path, PID, CPU, Disk I/O, Wakeups/s) with a header row. The Tools Startup page renders the registration inventory paginated 20 entries per page for responsiveness, with every entry reachable through Previous/Next controls and a shown-of-total caption; the underlying inventory is never truncated.

## Empty states

An empty or unavailable surface is a card of the standard radius containing: the metric icon at 20 px on a 16% metric-tint tile; one headline line at 13 px `ink`; at most two lines of explanation at 12 px `ink-dim`. It states plainly what is unavailable and why, and never implies a measurement exists. No wall of prose; no paragraph longer than two lines.

## Controls

All controls share one anatomy: 28 px tall (at least 24 px), a readable 12 px `ink` label, 8 px radius on framed surfaces, transparent fill at rest, `card_hi` on hover, and an accent-coloured focus border. Enter or Space activates the control and stops propagation.

- **Action buttons** — framed controls: 1 px `line` border, `Button` role; a selected action shows the 12% accent tint. The neutral `accent` applies in Settings and Tools; History, Sound, and Projects actions use the metric accent.
- **Segmented controls** — mutually exclusive choices inside one bordered 8 px surface (`RadioGroup` with an `aria-label` and tab-group semantics) holding `RadioButton` segments with `aria_toggled`, zero gap, and no separate segment outlines or internal separators; the selected segment shows the 12% accent tint, and a reserved transparent border turns full accent on keyboard focus without layout shift. Used for appearance, launcher visibility, menu readout styles, History range and metric, the Tools page row, activity sort, baseline windows, package source and package sort (two separate groups), and process filters. Refresh, save, and other actions stay as separate buttons outside any radio group.

Boolean preferences (dashboard tab visibility, Overview favorites, compact layout) are compact rows: the name on the left and a two-segment control on the right (Shown/Hidden or On/Off). Tab-order rows show icon-only up and down buttons in a fixed slot, omitting the up button on the first row and the down button on the last.
- **Steppers** — a decrement control, a noninteractive current value, and an increment control inside one bordered 8 px group with the same no-internal-separator anatomy (alert thresholds, output volume). Arrow-only or icon-only controls carry context-rich localized ARIA labels such as "Move CPU up".

The footer uses one control style for all three actions (Settings, Processes, Tools): a 14 px icon and 12 px label on a 28 px-tall button with 8 px radius, a 1 px `line` border, transparent fill at rest, `card_hi` on hover, and the metric accent on keyboard focus. Settings sits left-aligned; the live app/process counts stay centred as quiet `ink_dim` metadata (not a control); Processes and Tools form a tight right-aligned group with an 8 px gap, flush to the content inset. Keyboard focus order follows visual order after the tabs.

## Menu bar

- Exactly one native `NSStatusItem` stays registered. Its default content is a 16 px Tidegauge wave icon. Clicking anywhere opens Open Dashboard, Check for Updates, Settings, and Quit. The update item is disabled in preview builds and live in production.
- Readings are opt-in through Settings, with `icon`, `value`, `graph`, and `both` styles. Existing preferences are preserved; Reset to icon only clears optional readings.
- Enabled metrics share one transparent template image. Labels use 9 px system medium and are font-measured once per label (22 px minimum, rounded up) so longer localized labels widen the slot once instead of resizing per sample; values use 11 px system medium with monospaced digits inside fixed-width slots. Spacing between readings is 10 px. Native template tint follows the menu bar and selected state.
- Value and graph readings include CPU, MEM (MINNE in Bokmål), GPU, DSK (DISK), NET (NETT), BAT (BATT), or PWR (EFF) labels. Sparklines are 24×10 px with a 1 px stroke and at most 60 real samples. Missing samples show a dash.
- The icon remains visible when all readings are off, even if its preference is Hidden. The native item is removed when the monitor exits.
- Every dashboard tab has a labeled, keyboard-accessible Settings button in the footer. Command-comma also opens Settings; global Command-Q quits from the dashboard. A per-user file lock prevents duplicate monitors. Settings provides keyboard-accessible choices and reports persistence errors.

## Artwork and documentation preview

The generated activity-bar app icon uses a graphite tile with silver and off-white bars. See [artwork](assets/README.md) for the source PNG, macOS icon package, and generation prompt. UI chrome uses flat colors without gradients.

The README dashboard screenshot is a native render with generic sample data. It demonstrates layout and styling rather than measured hardware activity.
