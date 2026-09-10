# SPDX-License-Identifier: MIT OR Apache-2.0
# andlock - Count Android-style unlock patterns on n-dimensional grids
# Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

"""Exercise the performance gate without depending on host timing noise."""

from pathlib import Path
import os
import subprocess
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import compare


class PerformanceGateTests(unittest.TestCase):
    def compare_samples(self, elapsed, rss, growth, outputs=(b"same counts", b"same counts")):
        args = SimpleNamespace(
            baseline=Path("baseline"), candidate=Path("candidate"), runs=3,
            timeout_seconds=30, max_time_growth_percent=5,
            max_rss_growth_kib=0, max_binary_growth_bytes=8,
        )

        def info(path):
            return {
                "path": str(path),
                "bytes": 100 + (growth if str(path) == "candidate" else 0),
                "sha256": "unused",
            }

        def measure(binary, _arguments, _max_length, _timeout):
            index = int(binary == "candidate")
            return {
                "elapsed_seconds": elapsed[index], "peak_rss_kib": rss[index],
                "launcher_peak_rss_kib": 10,
            }, outputs[index]

        with (
            patch.object(compare, "CASES", (("example", "3x7", 0, 11),)),
            patch.object(compare, "binary_info", side_effect=info),
            patch.object(compare, "measure", side_effect=measure) as measured,
        ):
            result = compare.compare(args)
        order = [call.args[0] for call in measured.call_args_list]
        self.assertEqual(order, ["baseline", "candidate", "candidate", "baseline"] * 2)
        self.assertEqual(len(result["cases"][0]["samples"]["candidate"]), args.runs)
        return result

    def test_accepted_limits_include_the_boundary(self):
        result = self.compare_samples((1.0, 1.05), (100, 100), 8)
        self.assertEqual(result["regressions"], [])

    def test_each_metric_can_fail_the_gate(self):
        for elapsed, rss, growth, message in [
            ((1.0, 1.06), (100, 100), 0, "elapsed time grew"),
            ((1.0, 1.0), (100, 101), 0, "peak RSS grew"),
            ((1.0, 1.0), (100, 100), 9, "binary grew"),
        ]:
            with self.subTest(metric=message):
                result = self.compare_samples(elapsed, rss, growth)
                self.assertEqual(len(result["regressions"]), 1)
                self.assertIn(message, result["regressions"][0])

    def test_launcher_memory_cannot_masquerade_as_target_peak(self):
        with self.assertRaisesRegex(RuntimeError, "Python launcher peak"):
            self.compare_samples((1.0, 1.0), (100, 10), 0)

    def test_count_differences_invalidate_the_comparison(self):
        with self.assertRaisesRegex(RuntimeError, "count output differs"):
            self.compare_samples((1.0, 1.0), (100, 100), 0, (b"before", b"after"))

    def test_measure_rejects_incomplete_failed_or_warning_runs(self):
        for code in [
            "print('0 1')",
            "print('0 1\\n1 2'); raise SystemExit(1)",
            "import sys; print('0 1\\n1 2'); print('warning', file=sys.stderr)",
            "print('0 1\\n0 1\\n1 2')",
        ]:
            with self.subTest(code=code):
                with self.assertRaises(RuntimeError):
                    compare.measure(sys.executable, ["-c", code], 1, 5)

    def test_measure_returns_complete_output_and_process_resources(self):
        sample, output = compare.measure(sys.executable, ["-c", "print('0 1\\n1 2')"], 1, 5)
        self.assertEqual(output, b"0 1\n1 2\n")
        self.assertGreater(sample["elapsed_seconds"], 0)
        self.assertGreater(sample["peak_rss_kib"], 0)

    def test_timeout_kills_and_reaps_the_child(self):
        with subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True,
        ) as child:
            with self.assertRaises(TimeoutError):
                compare.wait_for_child(child, 0.02)
            self.assertEqual(child.returncode, -compare.signal.SIGKILL)
            with self.assertRaises(ChildProcessError):
                os.wait4(child.pid, os.WNOHANG)

    def test_nonfinite_or_negative_limits_are_rejected(self):
        for value in ["nan", "inf", "-1"]:
            with self.subTest(value=value):
                with self.assertRaises(compare.argparse.ArgumentTypeError):
                    compare.nonnegative(value)


if __name__ == "__main__":
    unittest.main()
