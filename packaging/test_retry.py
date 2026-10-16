import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
HELPER = ROOT / "packaging" / "retry.sh"


class RetryTests(unittest.TestCase):
    def run_helper(self, failures, attempts="3"):
        """Run a command that fails `failures` times, then succeeds."""
        with tempfile.TemporaryDirectory() as temporary:
            counter = Path(temporary) / "count"
            counter.write_text("0", encoding="utf-8")
            command = (
                f'n=$(cat "{counter}"); echo $((n + 1)) > "{counter}"; '
                f'[ "$n" -ge {failures} ] || exit 68'
            )
            result = subprocess.run(
                ["bash", "-c", f'source "{HELPER}"; retry_network "Test step" bash -c \'{command}\''],
                env={**os.environ, "TIDEGAUGE_RETRY_ATTEMPTS": attempts, "TIDEGAUGE_RETRY_DELAY": "0"},
                capture_output=True,
                text=True,
            )
            return result, int(counter.read_text(encoding="utf-8"))

    def test_a_dropped_connection_is_retried(self):
        result, runs = self.run_helper(failures=2)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(runs, 3)
        self.assertIn("retrying", result.stderr)

    def test_a_lasting_failure_still_fails_with_its_exit_code(self):
        result, runs = self.run_helper(failures=10)
        self.assertEqual(result.returncode, 68)
        self.assertEqual(runs, 3)
        self.assertIn("failed after 3 attempts", result.stderr)

    def test_success_runs_once(self):
        result, runs = self.run_helper(failures=0)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(runs, 1)


if __name__ == "__main__":
    unittest.main()
