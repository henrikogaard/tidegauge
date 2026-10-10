---
name: test-gpui-macos-panel-app
description: How to test a Rust+GPUI floating NSPanel macOS app on a headless EC2 Mac VM — click quirks, menu-bar reveal, screenshot/pixel-verification tricks.
---

# Testing GPUI NSPanel macOS apps (e.g. Tidegauge) on a remote Mac VM

## Context
Applies to Rust+GPUI apps that run as `NSApplicationActivationPolicy::Accessory` (no Dock icon) with a titlebar-less NSPanel window and NSStatusItems in the macOS menu bar.

## Running / inspecting
- Binary: `target/debug/<name>`; check `pgrep -fl target/debug/<name>`. `ps -o lstart` vs binary `stat` mtime tells you if the running build matches HEAD.
- The panel doesn't steal focus on click; clicks still land. BUT the **first click after app launch is often consumed as activation** — verify state after each click and retry once before concluding a click failed.
- To identify which tab/button is under the cursor without clicking, hover: hover-state backgrounds reveal the real hitbox (useful when click coordinates are uncertain).
- Labels that switch to semibold when active repack their neighbors by a few px — re-measure click targets from the *current* screenshot after each tab switch rather than caching coordinates.
- Real screen resolution may differ from the automation coordinate space: `sips -g pixelWidth -g pixelHeight /tmp/cap.png` — e.g. tool space 1024x768 vs real 1600x1200 (factor 1.5625). Convert when correlating pixel analysis with click coords.

## Menu bar (NSStatusItems)
- If the foreground app is fullscreen, the macOS menu bar auto-hides: move the cursor to the top screen edge and wait ~1s to reveal it, then screenshot/zoom the top-right.
- Status item clicks send the action on both left/right mouse down — a plain click is enough to test the toggle action.

## Visual verification
- `screencapture -x -o /tmp/cap.png` gives full-res captures; the computer tool's `zoom` action also returns full-resolution regions — use it to read tiny labels and locate rendering artifacts.
- For precise element positions, analyze pixels: `pip3 install --user pillow` (the developer-tools Python interpreter), then scan for accent-colored pixels and cluster x-ranges — distinguishes painted positions from hitboxes and finds displaced/missing rendering (e.g. canvas painted at content origin instead of inside a card).
- GPUI `canvas()` paint callbacks must paint in bounds-aware coordinates; a classic bug is paths painting at the scroll-content origin — look for stray shapes stacked at the content's top-left plus empty interiors inside cards.

## Safety
- Destructive-looking buttons ("Stop", "Stop All…") may execute immediately with no confirmation — verify presence visually; do not click unless the effect is safe/intended.

These recovered notes describe an earlier environment and are not current setup instructions. Use README.md and CONTRIBUTING.md for the current project.
