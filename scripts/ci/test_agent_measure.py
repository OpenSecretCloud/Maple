"""Execution and artifact bounds for CI phase instrumentation."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("agent_measure.py")
SPEC = importlib.util.spec_from_file_location("agent_measure", SCRIPT)
measure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(measure)


class AgentMeasurementTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ci measurements ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.metrics = self.root / "target/ci-metrics"
        self.environment = {**os.environ, "GITHUB_RUN_ID": "test-run", "GITHUB_RUN_ATTEMPT": "1",
                            "GITHUB_JOB": "desktop", "GITHUB_STEP_SUMMARY": str(self.root / "github-summary"),
                            "PRIVATE_CI_TEST_SECRET": "must-not-be-in-metrics"}

    def invoke(self, *args, environment=None):
        return subprocess.run([sys.executable, str(SCRIPT), *args], cwd=self.root,
                              env=environment or self.environment, capture_output=True, text=True)

    def phase(self, exit_code=0):
        return self.invoke("--phase", "test-build", "--target", "target/debug-ci", "--",
                           sys.executable, "-c", f"import sys; print('child output'); sys.exit({exit_code})")

    def records(self):
        return [json.loads(line) for line in (self.metrics / "phases.jsonl").read_text().splitlines()]

    def test_failing_command_streams_output_records_duration_and_preserves_failure(self):
        result = self.phase(23)
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertIn("child output", result.stdout)
        record, = self.records()
        self.assertEqual(record["exit_code"], 23)
        self.assertEqual(record["phase"], "test-build")
        self.assertEqual(record["target"], "target/debug-ci")
        self.assertGreater(record["elapsed_seconds"], 0)
        contents = "".join(path.read_text() for path in self.metrics.iterdir() if path.is_file())
        self.assertNotIn("child output", contents)
        self.assertNotIn("sys.exit", contents)
        self.assertNotIn("must-not-be-in-metrics", contents)

    def test_missing_command_records_127(self):
        result = self.invoke("--phase", "missing", "--", "./missing-command")
        self.assertEqual(result.returncode, 127)
        self.assertEqual(self.records()[0]["exit_code"], 127)

    def test_restored_metrics_from_another_attempt_are_reset(self):
        self.assertEqual(self.phase(9).returncode, 9)
        next_attempt = {**self.environment, "GITHUB_RUN_ATTEMPT": "2"}
        result = self.invoke("--phase", "retry", "--", sys.executable, "-c", "pass", environment=next_attempt)
        self.assertEqual(result.returncode, 0, result.stderr)
        record, = self.records()
        self.assertEqual(record["phase"], "retry")
        self.assertEqual(record["context"]["run_attempt"], "2")

    def test_summary_counts_regular_files_once_without_metrics_or_symlinks(self):
        self.assertEqual(self.phase().returncode, 0)
        target = self.root / "target"
        (target / "payload").write_bytes(b"12345")
        (target / "nested").mkdir()
        (target / "nested/small").write_bytes(b"123")
        (target / "outside").symlink_to(self.root)
        (target / "file-link").symlink_to(target / "payload")
        result = self.invoke("--summarize", "target")
        self.assertEqual(result.returncode, 0, result.stderr)
        summary = json.loads((self.metrics / "summary.json").read_text())
        self.assertEqual(summary["targets"][0]["logical_bytes"], 8)
        self.assertEqual(summary["targets"][0]["regular_files"], 2)
        self.assertIsNone(summary["cache_archive_bytes"])
        self.assertIn("not measured", (self.root / "github-summary").read_text())

    def test_collection_limits_current_html_and_excludes_restored_reports_and_links(self):
        self.assertEqual(self.phase().returncode, 0)
        started = self.records()[0]["started_unix_ns"]
        timing_dir = self.root / "target/cargo-timings"
        timing_dir.mkdir()
        for index in range(12):
            report = timing_dir / f"cargo-timing-{index:02}.html"
            report.write_text(f"report {index}")
            os.utime(report, ns=(started + index + 1, started + index + 1))
        old = timing_dir / "cargo-timing-old.html"
        old.write_text("stale")
        os.utime(old, ns=(started - 1, started - 1))
        (timing_dir / "cargo-timing.html").write_text("duplicate alias")
        (timing_dir / "cargo-timing-link.html").symlink_to(old)
        result = self.invoke("--summarize", "target")
        self.assertEqual(result.returncode, 0, result.stderr)
        summary = json.loads((self.metrics / "summary.json").read_text())
        timings = summary["cargo_timings"]
        self.assertEqual(timings["available_current_reports"], 12)
        self.assertEqual(len(timings["reports"]), 8)
        retained = sorted(path.name for path in (self.metrics / "cargo-timings").iterdir())
        self.assertEqual(retained, [f"target-0-cargo-timing-{index:02}.html" for index in range(4, 12)])

    def test_timing_size_limits_apply_before_copying(self):
        self.assertEqual(self.phase().returncode, 0)
        started = self.records()[0]["started_unix_ns"]
        timing_dir = self.root / "target/cargo-timings"
        timing_dir.mkdir()
        for index, size in enumerate((6, 6, 6, 17)):
            report = timing_dir / f"cargo-timing-{index}.html"
            report.write_bytes(b"x" * size)
            os.utime(report, ns=(started + index + 1, started + index + 1))
        with mock.patch.dict(os.environ, self.environment), mock.patch.object(measure, "MAX_REPORT_BYTES", 16), \
                mock.patch.object(measure, "MAX_TOTAL_REPORT_BYTES", 10):
            result = measure.collect_timings([self.root / "target"], self.metrics, self.records())
        self.assertEqual(result["retained_bytes"], 6)
        self.assertEqual(len(result["reports"]), 1)

    def test_phase_rejects_invalid_name_and_missing_command(self):
        self.assertEqual(self.invoke("--phase", "bad\nname", "--", "echo").returncode, 2)
        self.assertEqual(self.invoke("--phase", "no-command").returncode, 2)
        self.assertFalse(self.metrics.exists())

    def test_instrumentation_failure_does_not_replace_command_failure(self):
        with mock.patch.object(measure, "prepare", return_value={}), \
                mock.patch.object(measure, "read_phases", return_value=[]), \
                mock.patch.object(measure.subprocess, "run", return_value=subprocess.CompletedProcess([], 29)):
            self.assertEqual(measure.run_phase("failure", Path("target"), ["unused"], self.metrics), 29)


if __name__ == "__main__":
    unittest.main()
