# Apps and surviving helpers

Open **Processes** from the dashboard footer. **Remaining helpers** shows only processes whose ancestry to a main app was observed during this monitoring session and whose main app has positively exited. **All apps** also includes processes associated by executable bundle path. Bundle paths group rows; they do not establish which app instance launched a process or whether that process outlived one. Expand a group to see each PID, executable path, CPU, memory, and attribution evidence.

This is not an automatic orphan detector. Login items, updater agents, and background services may intentionally survive an app. A parent PID of 1 is not evidence of a leak. Unattributed standalone processes remain in the existing metric lists; the app-scoped inspector does not claim to show all operating-system processes.

## Evidence and lifetime

- Bundle paths establish association only. They never create a lifecycle origin or main-process identity. A detached bundle-only helper remains lifecycle-unknown and cannot be stopped here.
- Nested helper bundles roll up to the outer app group. When multiple app instances share a bundle, all registered main processes are preserved; the group does not choose an arbitrary instance.
- A helper origin is created only when sampled process ancestry reaches an identified running app process. Retained and inherited origins must have observed ancestry.
- An observed origin remains attached to its original main-process identity after the helper is reparented or the app is relaunched. It is not reassigned to another instance of the same bundle.
- Identities use PID plus native process start time. PID reuse cannot inherit a previous process's origin.
- Remaining status requires positive exit evidence for the observed main identity: a changed valid native identity, zombie state, or ESRCH. Missing or inaccessible samples alone are not exit evidence.
- Tracking is in memory, bounded by enumerated live PIDs, and resets when Tidegauge quits. A helper already detached when monitoring starts may have no recoverable origin. There is no persisted process-ancestry log.
- This inspector partitions its own rows; it does not add a second copy of a helper to recorded metrics or change historical app groups.

## Actions

**Quit app…** uses `NSRunningApplication.terminate`, a normal quit request, and is offered only when exactly one main app instance is registered for the bundle. If multiple instances are running, quit them from their own app windows. An app may show a save dialog, decline, or continue running. Tidegauge reports the observed result rather than treating request delivery as proof of exit.

**Stop…** is limited to individually selected, verified remaining helpers with observed ancestry and owned by the current user. It confirms the executable/PID/app association, warns about data loss/restarts, rechecks the latest observed origin and exited main, then validates native PID/start time, real/effective ownership and executable immediately before SIGTERM. Protected system paths, root, other users, PID 1, and Tidegauge itself are refused. Unknown or still-active-app processes cannot be stopped from this inspector.

Cancel sends nothing. There is no bulk kill, automatic cleanup, force quit, SIGKILL fallback, privileged helper, or disabling of launch agents. A short post-request observation distinguishes exit from still-running/unverifiable outcomes. macOS PID-based signals retain a small unavoidable race between final validation and delivery; this is not a claim of atomic termination.

## Verification

`cargo test --locked process_activity::` covers observed external ancestry, reparenting, missing-sample conservatism, PID reuse, preexisting daemons, bundle-only unknown lifecycle with multiple app instances, ancestry retained across relaunch, and protected targets. Native QA must use owned disposable app/helpers only: observe the group before quit, cancel a request, quit the parent, inspect the surviving child, cancel Stop, then explicitly stop that child. Never test termination against unrelated user applications.
