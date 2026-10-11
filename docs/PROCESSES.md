# Apps and surviving helpers

Open **Processes** from the dashboard footer. **Remaining helpers** shows only processes that outlived a main app process Tidegauge observed during this monitoring session. **All apps** also includes running apps and processes associated by executable bundle path. Expand a group to see each PID, executable path, CPU, memory, and attribution evidence.

This is not an automatic orphan detector. Login items, updater agents, and background services may intentionally survive an app. A parent PID of 1 is not evidence of a leak. Unattributed standalone processes remain in the existing metric lists; the app-scoped inspector does not claim to show all operating-system processes.

## Evidence and lifetime

- Bundle paths establish app association, not launch ancestry. Nested helper bundles roll up to the outer app.
- External helpers are attributed only through observed ancestry (or previously tracked ancestors). The original main-process identity stays attached to that live process even after reparenting or app relaunch.
- Identities use PID plus native process start time. PID reuse cannot inherit a previous process's origin.
- Remaining status requires positive exit evidence for the observed main identity: a changed valid native identity, zombie state, or ESRCH. Missing or inaccessible samples alone are not exit evidence.
- Tracking is in memory, bounded by enumerated live PIDs, and resets when Tidegauge quits. A helper already detached when monitoring starts may have no recoverable origin. There is no persisted process-ancestry log.
- This inspector partitions its own rows; it does not add a second copy of a helper to recorded metrics or change historical app groups.

## Actions

**Quit app…** uses `NSRunningApplication.terminate`, a normal quit request. The app may show a save dialog, decline, or continue running. Tidegauge reports the observed result rather than treating request delivery as proof of exit.

**Stop…** is limited to individually selected, verified remaining helpers owned by the current user. It confirms the executable/PID/app association, warns about data loss/restarts, rechecks the latest tracked origin and exited main, then validates native PID/start time, real/effective ownership and executable immediately before SIGTERM. Protected system paths, root, other users, PID 1, and Tidegauge itself are refused. Unknown or still-active-app processes cannot be stopped from this inspector.

Cancel sends nothing. There is no bulk kill, automatic cleanup, force quit, SIGKILL fallback, privileged helper, or disabling of launch agents. A short post-request observation distinguishes exit from still-running/unverifiable outcomes. macOS PID-based signals retain a small unavoidable race between final validation and delivery; this is not a claim of atomic termination.

## Verification

`cargo test --locked process_activity::` covers observed external ancestry, reparenting, missing-sample conservatism, PID reuse, preexisting daemons, bundle-vs-ancestry provenance, app relaunch, and protected targets. Native QA must use owned disposable app/helpers only: observe the group before quit, cancel a request, quit the parent, inspect the surviving child, cancel Stop, then explicitly stop that child. Never test termination against unrelated user applications.
