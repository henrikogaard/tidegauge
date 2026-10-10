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
only the versioned ZIP and checksum plus `appcast.xml` and its checksum. The
workflow never publishes the draft automatically.

Configure these seven GitHub Actions secrets before dispatch:

- `TIDEGAUGE_SPARKLE_PRIVATE_KEY`
- `APPLE_CERTIFICATE_P12_BASE64`
- `APPLE_CERTIFICATE_PASSWORD`
- `DEVELOPER_ID_APPLICATION`
- `APPLE_ID`
- `APPLE_TEAM_ID`
- `APPLE_APP_SPECIFIC_PASSWORD`

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

## Upgrade acceptance

End-to-end delivery is not claimed by this repository. Before a real release,
perform this bounded two-version test with two locally built bundles:

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
