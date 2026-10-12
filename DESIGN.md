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
- **Overview**: six 208 px metric cards in three columns (154 px in the user-toggleable `compact_dashboard` mode), 12 px radius, 16 px gaps; 34 px headline with a separate unit, three supporting readings, and a 38 px tinted strip holding a filled sparkline. Clicking a card opens its detail tab.
- **Hero card**: caption, 42 px number, sub-caption, filled area chart (130 px), footer stat triplets.
- **App rows**: rank, icon (rounded tile or cached app icon), name + `N processes`, usage bar, right-aligned value.
- **Donuts**: convex ring segments (inner radius 0.70), 92 px chart with a center value and subtitle, legend beside the chart with actual amounts and app icons. App breakdowns show the top four entries and Other. Donut cards pad 16 px with 12 px gaps.
- **Fonts**: `.AppleSystemUIFont`; captions uppercase 10 px semibold, body 12–14 px, hero number 42 px bold. Explanatory sentences and honesty disclaimers stay at 12 px.
- **Spacing**: 4/8/12/16 grid; overview padding 18–20 px; lists always expanded.

Overview sparklines share the detail-tab area chart and show the same recent samples. CPU/GPU averages and peaks cover the current session; network totals are interface counters since boot. Unavailable hardware readings show a dash. Per-app power remains labeled as estimated.

## Detail-page template

Every metric tab composes the same ordered parts with 16 px between sections:

1. **Hero card** — uppercase 10 px caption in the metric accent; 42 px bold value with the unit as separate smaller text; one sub-caption line for hardware/context; filled area chart (130 px) with its scale label top-right and older/now labels at the ends; a footer row of 3–4 stat triplets.
2. **Attribution section** — a standard section header (uppercase 10 px `ink-dim` label left, optional qualifier right) followed by app rows in the anatomy above. Present only where genuine per-app data exists.
3. **Supplementary sections** — same section-header pattern, each in a 12 px-radius card: CPU → temperatures and fans; Disk → devices and volumes; Battery → battery health and electrical details; GPU → Neural Engine; Sound → output control; Bluetooth → devices.

## Empty states

An empty or unavailable surface is a card of the standard radius containing: the metric icon at 20 px on a 16% metric-tint tile; one headline line at 13 px `ink`; at most two lines of explanation at 12 px `ink-dim`. It states plainly what is unavailable and why, and never implies a measurement exists. No wall of prose; no paragraph longer than two lines.

## Controls

Mutually exclusive choices use one segmented control (appearance, history ranges, package sources, baseline windows). Increment/decrement pairs form a single bordered group with the value between them. Actions are bordered buttons with 8 px radius. Every control is at least 24 px tall, keyboard focusable, and shows a metric-coloured focus border.

The footer uses one control style for all three actions (Settings, Processes, Tools): a 14 px icon and 12 px label on a 28 px-tall button with 8 px radius, a 1 px `line` border, transparent fill at rest, `card_hi` on hover, and the metric accent on keyboard focus. Settings sits left-aligned; the live app/process counts stay centred as quiet `ink_dim` metadata (not a control); Processes and Tools form a tight right-aligned group with an 8 px gap, flush to the content inset. Keyboard focus order follows visual order after the tabs.

## Menu bar

- Exactly one native `NSStatusItem` stays registered. Its default content is a 16 px Tidegauge wave icon. Clicking anywhere opens Open Dashboard, Check for Updates, Settings, and Quit. The update item is disabled in preview builds and live in production.
- Readings are opt-in through Settings, with `icon`, `value`, `graph`, and `both` styles. Existing preferences are preserved; Reset to icon only clears optional readings.
- Enabled metrics share one transparent template image. Labels use 9 px system medium, values use 11 px system medium with monospaced digits, and slots have fixed widths. Spacing between readings is 10 px. Native template tint follows the menu bar and selected state.
- Value and graph readings include CPU, MEM, GPU, DSK, NET, BAT, or PWR labels. Sparklines are 24×10 px with a 1 px stroke and at most 60 real samples. Missing samples show a dash.
- The icon remains visible when all readings are off, even if its preference is Hidden. The native item is removed when the monitor exits.
- Every dashboard tab has a labeled, keyboard-accessible Settings button in the footer. Command-comma also opens Settings; global Command-Q quits from the dashboard. A per-user file lock prevents duplicate monitors. Settings provides keyboard-accessible choices and reports persistence errors.

## Artwork and documentation preview

The generated activity-bar app icon uses a graphite tile with silver and off-white bars. See [artwork](assets/README.md) for the source PNG, macOS icon package, and generation prompt. UI chrome uses flat colors without gradients.

The README dashboard screenshot is a native render with generic sample data. It demonstrates layout and styling rather than measured hardware activity.
