# Releasing Tidegauge

This project packages a native macOS app bundle and uses Sparkle 2.9.6 for
signed update checks. Packaging never publishes a release or creates a tag.

## Pinned updater distribution

The package helper downloads this exact archive over HTTPS:

```text
https://github.com/sparkle-project/Sparkle/releases/download/2.9.6/Sparkle-2.9.6.tar.xz
SHA-256: 52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192
```

The archive is cached under `.cache/sparkle/` and its checksum is verified
before extraction. The complete framework distribution is embedded in
`Tidegauge.app/Contents/Frameworks/`, including its license in
`Contents/Resources/Sparkle-LICENSE.txt`.

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
export TIDEGAUGE_QA_FEED_URL=https://qa.example.invalid/appcast.xml
export TIDEGAUGE_QA_PUBLIC_KEY='<base64 32-byte Ed25519 public key>'
packaging/package-app.sh qa
```

The QA feed must use HTTPS and must not be the production feed. Replace the
example URL with a real test feed before QA; no test key or feed is stored in
this repository.

Production packaging fails closed unless these are present and valid:

```sh
export TIDEGAUGE_SPARKLE_PUBLIC_KEY='<base64 32-byte Ed25519 public key>'
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

After an optimized build, the release helper signs the nested framework code
before signing the app, creates a symlink-preserving archive with `ditto`,
generates a Sparkle EdDSA appcast, and writes SHA-256 files:

```sh
cargo build --release
packaging/release.sh
```

`release.sh` defaults to production and therefore stops before packaging
without the production public key, Developer ID identity, and an explicit
EdDSA signing key. Sparkle's appcast tool reads the private key from the
configured keychain account, defaulting to `ed25519`, or from the
`TIDEGAUGE_SPARKLE_PRIVATE_KEY` environment variable through standard input.
Private key material is never printed or written by these scripts. Set
`TIDEGAUGE_SPARKLE_SIGNING_ACCOUNT` to override the keychain account.
`NOTARY_PROFILE` is optional:

```sh
export NOTARY_PROFILE='configured-notarytool-profile'
packaging/release.sh
```

When set, the helper submits the archive, waits for notarization, and staples
the ticket. Without it, the output is signed but explicitly not notarized.
The helper does not upload, publish, tag, or mutate any external service.

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
