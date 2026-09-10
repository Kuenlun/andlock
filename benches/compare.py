#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# andlock - Count Android-style unlock patterns on n-dimensional grids
# Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

"""Compare exact counts, elapsed time, peak RSS, and size of two Linux builds."""

import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import resource
import signal
import statistics
import subprocess
import sys
import tempfile
import time


CASES = (
    ("3x7_l12", "3x7", 0, 12),
    ("3x7_f1_l11", "3x7", 1, 11),
    ("3x10_l8", "3x10", 0, 8),
    ("3x11_l8", "3x11", 0, 8),
)


def nonnegative(value):
    number = float(value)
    if not math.isfinite(number) or number < 0:
        raise argparse.ArgumentTypeError("must be a finite nonnegative number")
    return number


def binary_info(path):
    binary = path.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"not an executable file: {binary}")
    return {
        "path": str(binary),
        "bytes": binary.stat().st_size,
        "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
    }


def wait_for_child(child, timeout):
    def expired(_signum, _frame):
        raise TimeoutError(f"process exceeded {timeout:g} seconds")

    previous_handler = signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, timeout)
    try:
        try:
            _, status, usage = os.wait4(child.pid, 0)
        except BaseException:
            # The child owns a new process group. Stop descendants as well,
            # and reap the child before propagating timeout or interruption.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            _, status, _ = os.wait4(child.pid, 0)
            child.returncode = os.waitstatus_to_exitcode(status)
            raise
        child.returncode = os.waitstatus_to_exitcode(status)
        return usage
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous_handler)


def measure(binary, arguments, max_length, timeout):
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        launcher_peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        start = time.perf_counter_ns()
        with subprocess.Popen(
            [binary, *arguments], stdout=stdout, stderr=stderr, start_new_session=True,
        ) as child:
            usage = wait_for_child(child, timeout)
            elapsed = (time.perf_counter_ns() - start) / 1_000_000_000
        stdout.seek(0)
        stderr.seek(0)
        output = stdout.read()
        errors = stderr.read()
        if child.returncode != 0 or errors:
            raise RuntimeError(
                f"{binary} {' '.join(arguments)}: exit {child.returncode}, "
                f"stderr={errors.decode(errors='replace').strip()!r}"
            )
        lengths = []
        for line in output.splitlines():
            columns = line.split()
            if len(columns) == 2 and all(column.isdigit() for column in columns):
                lengths.append(int(columns[0]))
        if lengths != list(range(max_length + 1)):
            raise RuntimeError(f"{binary}: incomplete or invalid count table")
        return {
            "elapsed_seconds": elapsed,
            "peak_rss_kib": usage.ru_maxrss,
            "launcher_peak_rss_kib": launcher_peak,
        }, output


def compare(args):
    builds = {"baseline": binary_info(args.baseline), "candidate": binary_info(args.candidate)}
    report = {
        "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "platform": platform.platform(),
        "cpu_affinity": sorted(os.sched_getaffinity(0)),
        "builds": builds,
        "limits": {
            "time_growth_percent": args.max_time_growth_percent,
            "rss_growth_kib": args.max_rss_growth_kib,
            "binary_growth_bytes": args.max_binary_growth_bytes,
            "run_timeout_seconds": args.timeout_seconds,
        },
        "cases": [],
        "regressions": [],
    }
    size_growth = builds["candidate"]["bytes"] - builds["baseline"]["bytes"]
    if size_growth > args.max_binary_growth_bytes:
        report["regressions"].append(f"binary grew by {size_growth} bytes")
    for name, grid, free_points, max_length in CASES:
        arguments = [
            grid, "--free-points", str(free_points), "--max-length", str(max_length),
            "--memory-limit", "1GiB", "--quiet",
        ]
        samples = {"baseline": [], "candidate": []}
        expected_output = None
        # One unmeasured warmup per build, then alternate order to limit drift.
        for run in range(args.runs + 1):
            order = ("baseline", "candidate") if run % 2 == 0 else ("candidate", "baseline")
            for label in order:
                sample, output = measure(
                    builds[label]["path"], arguments, max_length, args.timeout_seconds,
                )
                if sample["peak_rss_kib"] <= sample["launcher_peak_rss_kib"]:
                    raise RuntimeError(
                        f"{name}: process RSS does not exceed the Python launcher peak"
                    )
                if expected_output is None:
                    expected_output = output
                elif output != expected_output:
                    raise RuntimeError(f"{name}: count output differs between runs or builds")
                if run != 0:
                    samples[label].append(sample)
        medians = {
            label: {
                metric: statistics.median(sample[metric] for sample in measurements)
                for metric in ("elapsed_seconds", "peak_rss_kib")
            }
            for label, measurements in samples.items()
        }
        before, after = medians["baseline"], medians["candidate"]
        time_growth = (after["elapsed_seconds"] / before["elapsed_seconds"] - 1) * 100
        rss_growth = after["peak_rss_kib"] - before["peak_rss_kib"]
        if after["elapsed_seconds"] > before["elapsed_seconds"] * (
            1 + args.max_time_growth_percent / 100
        ):
            report["regressions"].append(f"{name}: elapsed time grew by {time_growth:.2f}%")
        if rss_growth > args.max_rss_growth_kib:
            report["regressions"].append(f"{name}: peak RSS grew by {rss_growth} KiB")
        report["cases"].append({
            "name": name,
            "arguments": arguments,
            "samples": samples,
            "medians": medians,
            "time_growth_percent": time_growth,
            "rss_growth_kib": rss_growth,
        })
    for label, build in builds.items():
        if binary_info(Path(build["path"])) != build:
            raise RuntimeError(f"{label}: executable changed during the comparison")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--runs", type=int, default=5, help="measured runs per build (default: 5)")
    parser.add_argument("--timeout-seconds", type=nonnegative, default=30.0)
    parser.add_argument("--max-time-growth-percent", type=nonnegative, default=5.0)
    parser.add_argument("--max-rss-growth-kib", type=nonnegative, default=0)
    parser.add_argument("--max-binary-growth-bytes", type=nonnegative, default=0)
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("Linux is required for wait4 peak RSS in KiB")
    if args.timeout_seconds == 0:
        parser.error("--timeout-seconds must be positive")
    if args.runs < 3 or args.runs % 2 == 0:
        parser.error("--runs must be an odd integer of at least 3")
    try:
        report = compare(args)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(2, f"error: {error}\n")
    print(json.dumps(report, indent=2))
    return int(bool(report["regressions"]))


if __name__ == "__main__":
    sys.exit(main())
