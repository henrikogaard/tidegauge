# Tidegauge historical technical notes

Rust + gpui-kit 0.7.1 menu-bar system monitor. Repo at ~/repos/tidegauge.

## Key facts
- `proc_pid_rusage` returns **0 on success** (not byte count) — check `>= 0`. `proc_pidinfo` returns byte count — check `> 0`.
- `define_class!` classes must be referenced via `TideGaugeTarget::class()` (ClassType trait), NOT `objc2::class!(TideGaugeTarget)` — the latter does runtime lookup and panics "class could not be found".
- GPUI menu-bar popover = `WindowKind::PopUp` (NSPanel, nonactivating, joins all spaces). `AnyWindowHandle::update` consumes self — clone handle first.
- Path fill-only in gpui canvas: strokes are drawn as thin-quad Paths (see ui.rs area_chart/donut).
- AppGroup keys: bundle path may itself end in ".app" (no trailing slash) — `bundle_root()` searches for ".app/" so handle `key.ends_with(".app")` separately or names show full paths.
- Bundled app works via `scripts/bundle.sh` → `Tidegauge.app` (LSUIElement, ad-hoc codesign).
- No GitHub remote: gh can't create repos in Henrik's personal account from this box.
- VM: no battery/GPU/ANE/BT hardware — those tabs degrade gracefully.
- Data dirs: `~/Library/Application Support/no.ogard.tidegauge/` (settings.json, history.db), `~/Library/Caches/no.ogard.tidegauge/icons/`.

These recovered notes describe an earlier environment and are not current setup instructions. Use README.md and CONTRIBUTING.md for the current project.
