import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "packaging" / "create-dmg.sh"


class CreateDmgTests(unittest.TestCase):
    def run_script(self, fail_tool=None, existing=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            app = root / "Tidegauge.app"
            (app / "Contents").mkdir(parents=True)
            (app / "Contents" / "Info.plist").write_text("plist", encoding="utf-8")
            destination = root / "Tidegauge.dmg"
            if existing:
                destination.write_text("old", encoding="utf-8")
            fake_bin = root / "bin"
            fake_bin.mkdir()
            tmpdir = root / "tmp"
            tmpdir.mkdir()
            capture = root / "capture"
            capture.mkdir()
            (fake_bin / "ditto").write_text(
                '#!/bin/bash\nset -e\nsrc="${@: -2:1}"\ndst="${@: -1}"\n'
                'if [ "${FAIL_TOOL:-}" = "ditto" ]; then exit 1; fi\n'
                'cp -R "$src" "$dst"\n',
                encoding="utf-8",
            )
            (fake_bin / "hdiutil").write_text(
                '#!/bin/bash\nset -e\nif [ "${FAIL_TOOL:-}" = "hdiutil" ]; then exit 1; fi\n'
                'out="${@: -1}"\n'
                'src=""\nwhile [ "$#" -gt 0 ]; do\n'
                '  if [ "$1" = "-srcfolder" ]; then shift; src="$1"; fi\n'
                '  shift\ndone\n'
                'test "$(readlink "$src/Applications")" = /Applications\n'
                'test -d "$src/Tidegauge.app"\n'
                'cmp "$src/.DS_Store" "$TEST_LAYOUT"\n'
                'printf "%s" "$(readlink "$src/Applications")" > "$TEST_LINK"\n'
                'cp "$src/.DS_Store" "$TEST_CAPTURE_LAYOUT"\n'
                'printf dmg > "$out"\n',
                encoding="utf-8",
            )
            (fake_bin / "codesign").write_text(
                '#!/bin/bash\nif [ "${FAIL_TOOL:-}" = "codesign" ]; then exit 1; fi\nexit 0\n',
                encoding="utf-8",
            )
            for tool in ("ditto", "hdiutil", "codesign"):
                (fake_bin / tool).chmod(0o755)
            env = os.environ.copy()
            env["PATH"] = f"{fake_bin}{os.pathsep}{env['PATH']}"
            env["TMPDIR"] = str(tmpdir)
            env["TEST_LAYOUT"] = str(ROOT / "packaging" / "dmg-layout.DS_Store")
            env["TEST_LINK"] = str(capture / "Applications")
            env["TEST_CAPTURE_LAYOUT"] = str(capture / ".DS_Store")
            if fail_tool:
                env["FAIL_TOOL"] = fail_tool
            if fail_tool == "codesign":
                env["DEVELOPER_ID_APPLICATION"] = "test identity"
            result = subprocess.run(
                ["/bin/bash", str(SCRIPT), str(app), str(destination)],
                env=env,
                capture_output=True,
                text=True,
            )
            destination_contents = (
                destination.read_text(encoding="utf-8")
                if destination.exists()
                else None
            )
            stage_clean = list(tmpdir.iterdir()) == []
            link_path = capture / "Applications"
            layout_path = capture / ".DS_Store"
            link = link_path.read_text(encoding="utf-8") if link_path.exists() else None
            layout = layout_path.read_bytes() if layout_path.exists() else None
            workdirs_clean = not list(root.glob(".tidegauge-dmg.*"))
            return result, destination_contents, stage_clean, link, layout, workdirs_clean

    def test_stages_bundle_and_applications_shortcut_then_cleans_up(self):
        result, destination, stage_clean, link, layout, workdirs_clean = (
            self.run_script()
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(destination, "dmg")
        self.assertTrue(stage_clean)
        self.assertEqual(link, "/Applications")
        self.assertEqual(layout, (ROOT / "packaging" / "dmg-layout.DS_Store").read_bytes())
        self.assertTrue(workdirs_clean)

    def test_refuses_existing_destination(self):
        result, destination, stage_clean, _, _, workdirs_clean = self.run_script(
            existing=True
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(destination, "old")
        self.assertTrue(stage_clean)
        self.assertTrue(workdirs_clean)

    def test_cleans_staging_after_tool_failure(self):
        result, destination, stage_clean, _, _, workdirs_clean = self.run_script(
            fail_tool="hdiutil"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(destination)
        self.assertTrue(stage_clean)
        self.assertTrue(workdirs_clean)

    def test_failed_image_signing_does_not_publish_destination(self):
        result, destination, stage_clean, _, _, workdirs_clean = self.run_script(
            fail_tool="codesign"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(destination)
        self.assertTrue(stage_clean)
        self.assertTrue(workdirs_clean)


if __name__ == "__main__":
    unittest.main()
