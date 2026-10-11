# Releasing Tidegauge

This project packages a native macOS app bundle and uses Sparkle 2.9.6 for
signed update checks. Local packaging never publishes a GitHub release or
creates a tag. The manual release workflow creates a tagged draft only.

## Pinned updater distribution

The package helper downloads this exact archive over HTTPS:

```text
https://github.com/sparkle-project/Sparkle/releases/download/2.9.6/Sparkle-2.9.6.tar.xz
SHA-256: 52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192
```

The verified archive is freshly extracted under
`.cache/sparkle/distributions/2.9.6/` on each fetch. Packaging takes the
framework, command-line tools, and license only from that versioned
distribution, so an unrelated stale framework cannot be reused. The complete
framework distribution is embedded in `Tidegauge.app/Contents/Frameworks/`,
including its license in `Contents/Resources/Sparkle-LICENSE.txt`.

The bundle uses `no.ogard.tidegauge`, `Tidegauge.app`, `tidegauge.icns`,
`LSUIElement`, and a macOS 11.0 minimum. The minimum matches the existing
GPUI-linked release binary on the supported arm64 toolchain.

## Bundle modes

The package helper takes one explicit mode:

```sh
packaging/package-app.sh preview
packaging/package-app.sh qa
packaging/package-app.sh production
```

All modes use the existing optimized binary at
`target/release/tidegauge`, unless `TIDEGAUGE_BINARY` is supplied. Set
`TIDEGAUGE_VERSION` and `TIDEGAUGE_BUILD_VERSION` to override the Cargo
version and numeric `CFBundleVersion`.

Preview bundles are ad-hoc signed and set `TidegaugeUpdaterMode=preview`,
disable automatic checks, omit feed and key settings, and show a disabled
Check for Updates… menu item. This mode is safe for local layout and bundle
smoke checks and is not an updater test.

QA bundles are ad-hoc signed and require both of these explicitly separate
test settings:

```sh
export TIDEGAUGE_QA_FEED_URL=https://tidegauge-qa.invalid/appcast.xml
export TIDEGAUGE_QA_PUBLIC_KEY='<base64 32-byte Ed25519 public key>'
packaging/package-app.sh qa
```

The QA feed must use HTTPS and must not be the production feed. Replace the
example URL with a real test feed before QA; no test key or feed is stored in
this repository.

Production packaging reads its public key from the committed
`packaging/sparkle-public-key.txt` by default. Set
`TIDEGAUGE_SPARKLE_PUBLIC_KEY` only for an intentional key rotation or
verification setup. Previews do not require a public key. Production packaging
also requires a Developer ID identity:

```sh
export DEVELOPER_ID_APPLICATION='Developer ID Application: ...'
```

The same signed app can be wrapped in a local drag-and-drop disk image:

```sh
packaging/create-dmg.sh build/production/Tidegauge.app build/Tidegauge.dmg
```

The image contains only `Tidegauge.app`, an `Applications` shortcut, and the
committed Finder layout. The helper refuses to overwrite an existing image and
does not modify the source bundle. Release automation signs and notarizes the
DMG, staples its ticket, and validates it before producing the checksum.

To regenerate the deterministic Finder metadata during development, install
the optional `ds_store==1.3.2` package in a local virtual environment and run:

```sh
python3.12 -m venv "$HOME/.venvs/tidegauge-dmg-layout"
"$HOME/.venvs/tidegauge-dmg-layout/bin/pip" install 'ds_store==1.3.2'
"$HOME/.venvs/tidegauge-dmg-layout/bin/python" packaging/write-dmg-layout.py
```

This Python dependency is not used by release packaging; `create-dmg.sh` uses
the committed `packaging/dmg-layout.DS_Store`.

On macOS 13 and later, launch at login is opt-in and uses the native login-item
service. It can be enabled only when the signed app is installed in
`/Applications` or `~/Applications`; a pending item must be approved in
System Settings > General > Login Items. Settings offers System, Light, and
Dark appearance for the dashboard without changing the menu bar appearance.

The identity must be present in the local signing keychain. The production
feed is fixed to:

```text
https://github.com/henrikogaard/tidegauge/releases/latest/download/appcast.xml
```

Automatic checks are enabled only in production and QA bundles. Sparkle owns
update download and install consent; the app does not report a synthetic
success when Sparkle is unavailable.

## Signed release helper

After an optimized build, the release helper signs Sparkle's XPC services,
standalone `Autoupdate`, and `Updater.app` before signing the framework and
application. It preserves `Downloader.xpc` entitlements, creates a
symlink-preserving archive with `ditto`, generates a Sparkle EdDSA appcast, and
writes SHA-256 files:

```sh
cargo build --release
packaging/release.sh
```

`release.sh` defaults to production and requires the production public key and
Developer ID identity before packaging. It requires `NOTARY_PROFILE`; signed,
non-notarized output is not accepted. Sparkle's appcast tool reads the private
key from the configured keychain account, defaulting to
`no.ogard.tidegauge.sparkle`, or from `TIDEGAUGE_SPARKLE_PRIVATE_KEY` through
standard input. Set `TIDEGAUGE_SPARKLE_SIGNING_ACCOUNT` to override the
keychain account.

The release helper submits and staples the app, then validates the ticket with
`stapler validate` and assesses it with `spctl`. It isolates the current ZIP
as the only appcast input and generates version-specific enclosure URLs; the
feed URL in the bundle remains the stable `releases/latest/download/appcast.xml`
address. The candidate must pass both structural checks (a valid 64-byte
base64 EdDSA signature on every enclosure and the current archive's filename
and byte length) and CryptoKit signature verification against the public key
in the packaged `Info.plist`. The final appcast and its checksum are replaced
only after both validations succeed. Structural validation alone is not a
cryptographic signature check.

Private key material is passed to the appcast generator through standard input
and is never printed or written by these scripts. For local notarization,
configure a `notarytool` profile:

```sh
export NOTARY_PROFILE='configured-notarytool-profile'
# Optional when the profile is stored in a custom keychain:
export NOTARY_KEYCHAIN="$HOME/Library/Keychains/tidegauge-notary.keychain-db"
packaging/release.sh
```

The helper does not publish a GitHub release, upload release assets, or create
tags. Notarization contacts Apple's service when the required profile is
configured. `NOTARY_KEYCHAIN` is optional; when supplied, it selects the
keychain containing the notarization profile.

## GitHub release workflow

The `Packaging checks` workflow runs packaging Python tests and shell syntax
checks on pull requests and pushes to `main`. It does not build Rust code or
exercise macOS packaging and signing.

The manual `Draft release` workflow can be dispatched only from `main`. It
requires a numeric version newer than the latest published release and refuses
to proceed if that tag or a release already exists. It checks all required
secrets before the macOS build, asserts an Apple Silicon runner, runs the locked
Rust tests and release build, packages and notarizes the app, and creates a
draft release tagged `vVERSION` at the dispatched commit. The draft contains
the versioned ZIP and checksum, the matching DMG and checksum, plus
`appcast.xml` and its checksum. The
workflow never publishes the draft automatically.

Configure these GitHub Actions secrets before dispatch:

- `TIDEGAUGE_SPARKLE_PRIVATE_KEY`
- `APPLE_CERTIFICATE_P12_BASE64`
- `APPLE_CERTIFICATE_PASSWORD`
- `DEVELOPER_ID_APPLICATION`
- `APPLE_TEAM_ID`
- `APPSTORE_API_PRIVATE_KEY` (the App Store Connect API `.p8` file contents)

And configure these repository variables (not secret):

- `APPSTORE_API_KEY_ID`
- `APPSTORE_ISSUER_ID`

Notarization authenticates with that App Store Connect API key, not an Apple
ID and app-specific password.

The Sparkle public key is committed at `packaging/sparkle-public-key.txt`; it
is not a secret. The workflow imports the certificate into a temporary
keychain, grants noninteractive signing access, stores notarization credentials
there, removes the P12 file immediately after import, and deletes the keychain
even if the job fails.

Keep a secure offline backup of the stable private key. Do not replace it
casually: changing the key requires a planned update path for already-installed
clients. A draft release is not evidence of production update delivery; feed
hosting, publishing, and end-to-end update acceptance remain release-owner
responsibilities.

## Signing-key custody and recovery

The Ed25519 signing key is independent of GitHub. GitHub Actions stores a
deployment copy, not a recoverable backup: its API cannot return the private
value. Do not assume a saved automation secret can be revealed later.

Keep the raw private key in a concealed password field in the release owner's
password vault. Keep a portable password-encrypted `age` backup and its
strong, independently chosen transfer password in the vault as well, with an
appropriate offline recovery copy. This transfer password is not the vault's
account password. Never commit the private key, certificate exports, API
private keys, encrypted key backups, passwords, or vault recovery credentials.
Only `packaging/sparkle-public-key.txt` belongs in source control.

The password-encrypted backup is distinct from an older SSH-recipient backup:
it needs only its transfer password, not an SSH identity or a GitHub account.
On a trusted Mac with `age` installed, restore directly to the clipboard without
printing the key or creating a plaintext file:

```sh
key="$(age --decrypt "$HOME/Downloads/Tidegauge-Sparkle-password-backup.age")" && printf '%s' "$key" | pbcopy && unset key && echo "Key copied to clipboard"
```

Enter the transfer password at the secure terminal prompt. Continue only if
the command reports that the key was copied. Paste into the concealed vault
field or the `TIDEGAUGE_SPARKLE_PRIVATE_KEY` Actions secret, then clear the
clipboard with `pbcopy < /dev/null`. Never paste the encrypted file contents
into the Actions secret. Do not put private values in shell command history,
chat, screenshots, logs, issue bodies, or release attachments.

Keep the same key when moving CI or hosting. Verify a restored key against the
committed public key before accepting it; `release.sh` verifies the archive's
signature using the public key embedded in the packaged app and fails closed
on a mismatch. Key generation is not a recovery procedure. Rotation needs a
planned, tested update path for already-installed clients.

The current feed is hosted on GitHub. Before moving it, ship an update through
the old feed that points clients to the new HTTPS address; keep the old feed
available during migration. An update-feed address on an owner-controlled
domain is preferable before public distribution, but no such address is
configured here. A draft release does not make the production feed live.

## Upgrade acceptance

The published 0.2.0 release has a
[verified 0.1.0 candidate → 0.2.0 production-feed upgrade](https://github.com/henrikogaard/tidegauge/releases/download/v0.2.0/VERIFICATION.md).
That evidence does not cover future releases or invalid-signature rejection.
An isolated unreachable-feed error/recovery check also passed after 0.2.0.
Before a new release, perform this bounded two-version test with two locally
built bundles:

1. Install version N with a test feed and test Ed25519 key.
2. Publish only the version N+1 archive and signed appcast to the isolated QA
   feed.
3. Select Check for Updates… and confirm the native Sparkle consent flow.
4. Confirm the app relaunches as N+1, retains its bundle identifier and
   settings, and enables the next automatic check.
5. Confirm an invalid signature and an unreachable feed fail visibly without
   replacing version N.

Production notarization, feed hosting, key custody, and update delivery remain
release-owner responsibilities and require real credentials and an isolated
QA feed.

### Remaining acceptance and unblock steps

- **Native invalid-signature rejection ([#32](https://github.com/henrikogaard/tidegauge/issues/32)):**
  provide an isolated HTTPS endpoint whose certificate is trusted by the test
  Mac. First verify a valid QA update there. Then alter that test archive after
  signing, update its enclosure byte length if necessary, but retain the original
  signature. Confirm the failure is signature rejection, not TLS, parsing, or
  download failure, and that the installed app/version is unchanged. Use QA-only
  bundle/feed/key/profile settings; do not repoint production clients, weaken TLS
  trust, or reuse the production private key. The earlier local-certificate
  attempt did not reach signature verification.
- **Physical validation ([#31](https://github.com/henrikogaard/tidegauge/issues/31),
  [#8](https://github.com/henrikogaard/tidegauge/issues/8)):** use the installed
  signed build on the target Mac, recording model/chip/macOS/build. Compare
  battery, disk and sensor readings with independent native diagnostics; record
  unavailable channels honestly. Check idle/load and sleep/wake recovery. Test
  supported audible output controls with an owned sound source and restore its
  original volume/mute. Ask the user before actual sign-out/sign-in, and verify
  exactly one Tidegauge instance starts while settings and History are preserved.
- **Package mutation acceptance:** use a disposable macOS account or VM and
  disposable manager prefixes/packages. Confirm cancel sends no command,
  update/removal affects only the confirmed item, and stale identity, manager
  failure and partial outcomes remain explicit. Do not use the developer's real
  installed packages as fixtures.

Before publishing, reconcile [CHANGELOG.md](../CHANGELOG.md) with the artifact's
commit and actual version/date. Leave unmerged work under Unreleased, and do not
convert outstanding acceptance checks into release claims.
