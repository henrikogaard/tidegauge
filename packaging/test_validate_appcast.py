import base64
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path
from urllib.parse import quote

from validate_appcast import (
    AppcastValidationError,
    SIGNATURE_ATTRIBUTE,
    SPARKLE_NAMESPACE,
    validate_appcast,
)


REPO_ROOT = Path(__file__).resolve().parent.parent
SIGNATURE = base64.b64encode(bytes(range(64))).decode("ascii")


def make_archive(path):
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_STORED) as archive:
        archive.writestr("payload.txt", b"test archive")
    return Path(path)


def write_feed(path, enclosures):
    ET.register_namespace("sparkle", SPARKLE_NAMESPACE)
    root = ET.Element("rss")
    channel = ET.SubElement(root, "channel")
    for attributes in enclosures:
        item = ET.SubElement(channel, "item")
        ET.SubElement(item, "enclosure", attributes)
    ET.ElementTree(root).write(path, encoding="utf-8", xml_declaration=True)
    return Path(path)


def signed_enclosure(url, length):
    return {
        "url": url,
        "length": str(length),
        SIGNATURE_ATTRIBUTE: SIGNATURE,
    }


class AppcastValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        self.current_zip = make_archive(self.root / "Tidegauge 1.2.3.zip")
        self.current_url = (
            "https://updates.invalid/releases/" + quote(self.current_zip.name, safe="")
        )

    def tearDown(self):
        self.temp_dir.cleanup()

    def feed(self, enclosures):
        path = self.root / "appcast.xml"
        write_feed(path, enclosures)
        return path

    def test_accepts_signed_feed_representing_current_archive(self):
        appcast = self.feed(
            [signed_enclosure(self.current_url, self.current_zip.stat().st_size)]
        )
        validate_appcast(appcast, self.current_zip)

    def test_rejects_unsigned_enclosure_in_mixed_feed(self):
        appcast = self.feed(
            [
                signed_enclosure(self.current_url, self.current_zip.stat().st_size),
                {"url": "older.zip", "length": "42"},
            ]
        )
        with self.assertRaisesRegex(AppcastValidationError, "signature"):
            validate_appcast(appcast, self.current_zip)

    def test_rejects_malformed_and_wrong_length_signatures(self):
        for signature in ("not-base64!", base64.b64encode(bytes(63)).decode("ascii")):
            with self.subTest(signature_kind="malformed" if "!" in signature else "wrong length"):
                appcast = self.feed(
                    [
                        {
                            "url": self.current_url,
                            "length": str(self.current_zip.stat().st_size),
                            SIGNATURE_ATTRIBUTE: signature,
                        }
                    ]
                )
                with self.assertRaisesRegex(AppcastValidationError, "signature") as error:
                    validate_appcast(appcast, self.current_zip)
                self.assertNotIn(signature, str(error.exception))

    def test_rejects_feed_without_current_archive(self):
        appcast = self.feed([signed_enclosure("previous-release.zip", 42)])
        with self.assertRaisesRegex(AppcastValidationError, "current archive"):
            validate_appcast(appcast, self.current_zip)

    def test_rejects_wrong_current_archive_length(self):
        appcast = self.feed(
            [
                signed_enclosure(
                    self.current_url, self.current_zip.stat().st_size + 1
                )
            ]
        )
        with self.assertRaisesRegex(AppcastValidationError, "current archive"):
            validate_appcast(appcast, self.current_zip)

    @unittest.skipUnless(
        sys.platform == "darwin" and shutil.which("swift"),
        "CryptoKit verifier test requires macOS and Swift; skipped on this platform",
    )
    def test_crypto_verifier_accepts_valid_and_rejects_tampering_or_wrong_key(self):
        result = subprocess.run(
            [
                shutil.which("swift"),
                str(REPO_ROOT / "packaging/test_verify_appcast_signatures.swift"),
                str(REPO_ROOT / "packaging/verify_appcast_signatures.swift"),
                shutil.which("swift"),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_unsigned_generator_output_does_not_replace_existing_feed(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fixture_root = Path(temp_dir) / "fixture"
            packaging = fixture_root / "packaging"
            scripts = fixture_root / "scripts"
            fake_bin = fixture_root / "fake-bin"
            distribution = fixture_root / "distribution"
            output_root = fixture_root / "release-output"
            for directory in (packaging, scripts, fake_bin, distribution / "bin", output_root):
                directory.mkdir(parents=True, exist_ok=True)

            (fixture_root / "Cargo.toml").write_text(
                '[package]\nname = "tidegauge"\nversion = "1.2.3"\n',
                encoding="utf-8",
            )
            shutil.copyfile(REPO_ROOT / "packaging/release.sh", packaging / "release.sh")
            shutil.copyfile(REPO_ROOT / "packaging/retry.sh", packaging / "retry.sh")
            create_dmg = packaging / "create-dmg.sh"
            create_dmg.write_text(
                "#!/bin/bash\n"
                "set -euo pipefail\n"
                'mkdir -p "$(dirname "$2")"\n'
                'printf "mock dmg" > "$2"\n',
                encoding="utf-8",
            )
            create_dmg.chmod(0o755)
            shutil.copyfile(
                REPO_ROOT / "packaging/validate_appcast.py",
                packaging / "validate_appcast.py",
            )

            package_script = packaging / "package-app.sh"
            package_script.write_text(
                "#!/bin/bash\n"
                "set -euo pipefail\n"
                'mkdir -p "$TIDEGAUGE_OUTPUT_ROOT/production/Tidegauge.app/Contents"\n'
                'printf plist > "$TIDEGAUGE_OUTPUT_ROOT/production/Tidegauge.app/Contents/Info.plist"\n',
                encoding="utf-8",
            )
            package_script.chmod(0o755)

            fetch_script = scripts / "fetch-sparkle.sh"
            fetch_script.write_text(
                "#!/bin/bash\n"
                "set -euo pipefail\n"
                'printf "%s\\n" "$TEST_DISTRIBUTION"\n',
                encoding="utf-8",
            )
            fetch_script.chmod(0o755)

            generator = distribution / "bin/generate_appcast"
            generator.write_text(
                f"#!{sys.executable}\n"
                "import base64\n"
                "import os\n"
                "import pathlib\n"
                "import sys\n"
                "from urllib.parse import quote\n"
                "arguments = sys.argv[1:]\n"
                "output = pathlib.Path(arguments[arguments.index('-o') + 1])\n"
                "input_dir = pathlib.Path(arguments[-1])\n"
                "archive = next(input_dir.glob('*.zip'))\n"
                "url = quote(archive.name, safe='')\n"
                "signature = '' if os.environ.get('TEST_UNSIGNED_GENERATOR') == '1' else base64.b64encode(bytes(range(64))).decode('ascii')\n"
                "signature_attribute = '' if not signature else ' sparkle:edSignature=\"' + signature + '\"'\n"
                "output.write_text(\n"
                "    '<rss xmlns:sparkle=\"' + 'http://www.andymatuschak.org/xml-namespaces/sparkle' + '\"><channel><item><enclosure url=\"' + url\n"
                "    + '\" length=\"' + str(archive.stat().st_size)\n"
                "    + '\"' + signature_attribute + ' /></item></channel></rss>', encoding='utf-8')\n",
                encoding="utf-8",
            )
            generator.chmod(0o755)

            ditto_stub = fake_bin / "ditto"
            ditto_stub.write_text(
                "#!/bin/bash\n"
                "set -euo pipefail\n"
                'printf "mock archive" > "${@: -1}"\n',
                encoding="utf-8",
            )
            ditto_stub.chmod(0o755)

            shasum_stub = fake_bin / "shasum"
            shasum_stub.write_text(
                "#!/bin/bash\n"
                'printf "mock-checksum  %s\\n" "${@: -1}"\n',
                encoding="utf-8",
            )
            shasum_stub.chmod(0o755)

            xcrun_stub = fake_bin / "xcrun"
            xcrun_stub.write_text(
                "#!/bin/bash\n"
                'printf "%s\\n" "$*" >> "$TEST_XCRUN_LOG"\n'
                "exit 0\n",
                encoding="utf-8",
            )
            xcrun_stub.chmod(0o755)

            spctl_stub = fake_bin / "spctl"
            spctl_stub.write_text("#!/bin/bash\nexit 0\n", encoding="utf-8")
            spctl_stub.chmod(0o755)

            swift_stub = fake_bin / "swift"
            swift_stub.write_text(
                "#!/bin/bash\n"
                "set -euo pipefail\n"
                ': > "$TEST_VERIFY_MARKER"\n'
                'if [ "${FAIL_APPCAST_SIGNATURE_VERIFICATION:-0}" = "1" ]; then\n'
                '    printf "%s\\n" "Appcast signature verification failed" >&2\n'
                "    exit 1\n"
                "fi\n",
                encoding="utf-8",
            )
            swift_stub.chmod(0o755)

            old_archive = make_archive(output_root / "Tidegauge 1.2.2.zip")
            appcast = output_root / "appcast.xml"
            write_feed(
                appcast,
                [signed_enclosure(old_archive.name, old_archive.stat().st_size)],
            )
            validate_appcast(appcast, old_archive)
            previous_feed = appcast.read_bytes()
            checksum = output_root / "appcast.xml.sha256"
            previous_checksum = (
                hashlib.sha256(previous_feed).hexdigest() + f"  {appcast}\n"
            ).encode("utf-8")
            checksum.write_bytes(previous_checksum)

            base_environment = os.environ.copy()
            base_environment.pop("TIDEGAUGE_SPARKLE_PRIVATE_KEY", None)
            base_environment.pop("NOTARY_PROFILE", None)
            base_environment.update(
                {
                    "DEVELOPER_ID_APPLICATION": "test identity",
                    "PATH": os.pathsep.join((str(fake_bin), os.environ.get("PATH", ""))),
                    "TEST_DISTRIBUTION": str(distribution),
                    "TIDEGAUGE_OUTPUT_ROOT": str(output_root),
                    "TIDEGAUGE_SPARKLE_PUBLIC_KEY": "test public key",
                    "TIDEGAUGE_SPARKLE_SIGNING_ACCOUNT": "test-account",
                    "TIDEGAUGE_VERSION": "1.2.3",
                    "NOTARY_KEYCHAIN": str(fixture_root / "test-notary.keychain-db"),
                    "NOTARY_PROFILE": "test-notary-profile",
                    "TEST_VERIFY_MARKER": str(fixture_root / "verifier-called"),
                    "TEST_XCRUN_LOG": str(fixture_root / "xcrun.log"),
                }
            )
            for signing_mode in ("keychain", "private-key"):
                with self.subTest(signing_mode=signing_mode):
                    environment = base_environment.copy()
                    environment["TEST_UNSIGNED_GENERATOR"] = "1"
                    if signing_mode == "private-key":
                        environment["TIDEGAUGE_SPARKLE_PRIVATE_KEY"] = (
                            "test-only-dummy-private-key"
                        )
                    appcast.write_bytes(previous_feed)
                    checksum.write_bytes(previous_checksum)
                    result = subprocess.run(
                        ["/bin/bash", str(packaging / "release.sh")],
                        cwd=fixture_root,
                        env=environment,
                        capture_output=True,
                        text=True,
                        check=False,
                    )

                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    self.assertIn("Appcast validation failed", result.stderr)
                    self.assertEqual(appcast.read_bytes(), previous_feed)
                self.assertEqual(checksum.read_bytes(), previous_checksum)
                self.assertEqual(list(output_root.glob(".release.*")), [])
                self.assertEqual(list(output_root.glob(".notary.*")), [])

            with self.subTest(success="validated appcast and relative ZIP checksum"):
                environment = base_environment.copy()
                environment["TEST_UNSIGNED_GENERATOR"] = "0"
                Path(environment["TEST_XCRUN_LOG"]).write_text("", encoding="utf-8")
                zip_path = output_root / "Tidegauge-1.2.3.zip"
                zip_checksum = output_root / f"{zip_path.name}.sha256"
                dmg_path = output_root / "Tidegauge-1.2.3.dmg"
                dmg_checksum = output_root / f"{dmg_path.name}.sha256"
                result = subprocess.run(
                    ["/bin/bash", str(packaging / "release.sh")],
                    cwd=fixture_root,
                    env=environment,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotEqual(appcast.read_bytes(), previous_feed)
                validate_appcast(appcast, zip_path)
                self.assertEqual(
                    zip_checksum.read_text(encoding="utf-8"),
                    f"mock-checksum  {zip_path.name}\n",
                )
                self.assertNotIn(str(output_root), zip_checksum.read_text(encoding="utf-8"))
                self.assertEqual(
                    dmg_checksum.read_text(encoding="utf-8"),
                    f"mock-checksum  {dmg_path.name}\n",
                )
                self.assertIn(f"Installer: {dmg_path}", result.stdout)
                self.assertEqual(
                    list(output_root.glob(".release.*")),
                    [],
                )
                self.assertEqual(
                    list(output_root.glob(".notary.*")),
                    [],
                )
                self.assertIn(
                    f"--keychain {environment['NOTARY_KEYCHAIN']}",
                    Path(environment["TEST_XCRUN_LOG"]).read_text(encoding="utf-8"),
                )
                self.assertEqual(
                    checksum.read_text(encoding="utf-8"),
                    "mock-checksum  appcast.xml\n",
                )

            with self.subTest(success="relative output root containing spaces"):
                relative_root_name = "release output"
                relative_root = fixture_root / relative_root_name
                relative_environment = environment.copy()
                relative_environment["TIDEGAUGE_OUTPUT_ROOT"] = relative_root_name
                relative_zip = relative_root / "Tidegauge-1.2.3.zip"
                relative_dmg = relative_root / "Tidegauge-1.2.3.dmg"
                relative_dmg_checksum = relative_root / f"{relative_dmg.name}.sha256"
                result = subprocess.run(
                    ["/bin/bash", str(packaging / "release.sh")],
                    cwd=fixture_root,
                    env=relative_environment,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                validate_appcast(relative_root / "appcast.xml", relative_zip)
                self.assertEqual(
                    relative_dmg_checksum.read_text(encoding="utf-8"),
                    f"mock-checksum  {relative_dmg.name}\n",
                )
                self.assertFalse((relative_root / relative_root_name).exists())

            appcast.write_bytes(previous_feed)
            checksum.write_bytes(previous_checksum)
            with self.subTest(failure="missing notarization profile"):
                environment = base_environment.copy()
                environment.pop("NOTARY_PROFILE")
                Path(environment["TEST_VERIFY_MARKER"]).unlink(missing_ok=True)
                result = subprocess.run(
                    ["/bin/bash", str(packaging / "release.sh")],
                    cwd=fixture_root,
                    env=environment,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn("NOTARY_PROFILE", result.stderr)
                self.assertEqual(appcast.read_bytes(), previous_feed)
                self.assertEqual(checksum.read_bytes(), previous_checksum)
                self.assertFalse(Path(environment["TEST_VERIFY_MARKER"]).exists())

            appcast.write_bytes(previous_feed)
            checksum.write_bytes(previous_checksum)
            with self.subTest(failure="signature verification"):
                environment = base_environment.copy()
                environment["TEST_UNSIGNED_GENERATOR"] = "0"
                environment["FAIL_APPCAST_SIGNATURE_VERIFICATION"] = "1"
                marker = Path(environment["TEST_VERIFY_MARKER"])
                marker.unlink(missing_ok=True)
                result = subprocess.run(
                    ["/bin/bash", str(packaging / "release.sh")],
                    cwd=fixture_root,
                    env=environment,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn("Appcast signature verification failed", result.stderr)
                self.assertTrue(marker.exists())
                self.assertEqual(appcast.read_bytes(), previous_feed)
                self.assertEqual(checksum.read_bytes(), previous_checksum)
                self.assertEqual(list(output_root.glob(".release.*")), [])
                self.assertEqual(list(output_root.glob(".notary.*")), [])


if __name__ == "__main__":
    unittest.main()
