#!/usr/bin/env python3
"""Measure CI phases without retaining commands, private environment, or output.

Run from apps/maple-agent, for example:
  python3 ../../scripts/ci/agent_measure.py --phase build --target target -- cargo build --locked --timings
  python3 ../../scripts/ci/agent_measure.py --summarize target

The child inherits stdout/stderr. Only the final summary scans target trees;
logical file bytes are not compressed cache archive or network transfer bytes.
"""

import argparse
from datetime import datetime, timezone
import heapq
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time


MAX_PHASES = 256
MAX_PHASE_BYTES = 1024 * 1024
MAX_REPORTS = 8
MAX_REPORT_BYTES = 16 * 1024 * 1024
MAX_TOTAL_REPORT_BYTES = 32 * 1024 * 1024


def context():
    """Only public Actions identifiers belong in the measurement artifact."""
    return {name: os.environ.get(variable, "")[:128] for name, variable in (
        ("run_id", "GITHUB_RUN_ID"), ("run_attempt", "GITHUB_RUN_ATTEMPT"),
        ("job", "GITHUB_JOB"), ("runner_os", "RUNNER_OS"), ("runner_arch", "RUNNER_ARCH"),
    )}


def output_path(directory, name):
    path = directory / name
    if path.is_symlink():
        raise ValueError("A CI measurement output is a symlink")
    return path


def prepare(directory):
    if directory.is_symlink():
        raise ValueError("The CI measurement directory is a symlink")
    directory.mkdir(parents=True, exist_ok=True)
    marker = output_path(directory, "context.json")
    previous = None
    if marker.exists() and marker.stat().st_size < 4096:
        previous = json.loads(marker.read_text(encoding="utf-8"))
    current = context()
    if previous != current:
        # A restored target cache must not contribute another run's phases.
        output_path(directory, "phases.jsonl").write_text("", encoding="utf-8")
        marker.write_text(json.dumps(current) + "\n", encoding="utf-8")
    return current


def read_phases(directory):
    source = output_path(directory, "phases.jsonl")
    if not source.exists():
        return []
    if source.stat().st_size > MAX_PHASE_BYTES:
        raise ValueError("CI measurement history exceeds its byte limit")
    records = [json.loads(line) for line in source.read_text(encoding="utf-8").splitlines() if line]
    if len(records) > MAX_PHASES:
        raise ValueError("CI measurement history exceeds its phase limit")
    return records


def run_phase(phase, target, command, directory):
    current = prepare(directory)
    if len(read_phases(directory)) >= MAX_PHASES:
        raise ValueError("CI measurement history exceeds its phase limit")
    started_unix_ns = time.time_ns()
    started = time.monotonic()
    try:
        # Inherit both streams: no pipes, tee, or unbounded compiler log copy.
        result = subprocess.run(command, check=False)
        code = result.returncode if result.returncode >= 0 else 128 - result.returncode
    except FileNotFoundError:
        print("CI phase command was not found", file=sys.stderr)
        code = 127
    except PermissionError:
        print("CI phase command was not executable", file=sys.stderr)
        code = 126
    except KeyboardInterrupt:
        code = 130
    record = {
        "schema_version": 1, "context": current, "phase": phase, "target": str(target),
        "started_at": datetime.fromtimestamp(started_unix_ns / 1e9, timezone.utc).isoformat(),
        "started_unix_ns": started_unix_ns,
        "elapsed_seconds": round(time.monotonic() - started, 6), "exit_code": code,
    }
    try:
        serialized = json.dumps(record, sort_keys=True) + "\n"
        destination = output_path(directory, "phases.jsonl")
        if destination.stat().st_size + len(serialized.encode("utf-8")) > MAX_PHASE_BYTES:
            raise ValueError("CI measurement history exceeds its byte limit")
        with destination.open("a", encoding="utf-8") as stream:
            stream.write(serialized)
    except (OSError, ValueError):
        print("Failed to record CI phase measurements", file=sys.stderr)
        # Never replace a failing build's exit code with an instrumentation error.
        return code or 1
    print(f"CI phase {phase}: {record['elapsed_seconds']:.3f}s, exit {code}", flush=True)
    return code


def target_size(target, directory):
    result = {"target": str(target), "logical_bytes": 0, "regular_files": 0, "scan_errors": 0}
    if target.is_symlink():
        result["scan_errors"] = 1
        return result
    result["exists"] = target.is_dir()
    metrics_path = directory.absolute()

    def scan_error(_error):
        result["scan_errors"] += 1

    for root, folders, files in os.walk(target, followlinks=False, onerror=scan_error):
        folders[:] = [name for name in folders
                      if not (Path(root) / name).is_symlink()
                      and (Path(root) / name).absolute() != metrics_path]
        for name in files:
            try:
                info = (Path(root) / name).lstat()
                if stat.S_ISREG(info.st_mode):
                    result["logical_bytes"] += info.st_size
                    result["regular_files"] += 1
            except OSError:
                scan_error(None)
    return result


def collect_timings(targets, directory, phases):
    """Retain a bounded selection of current-run Cargo HTML timing reports."""
    destination = output_path(directory, "cargo-timings")
    destination.mkdir(exist_ok=True)
    for old in destination.iterdir():
        if old.name.endswith(".html") and (old.is_file() or old.is_symlink()):
            old.unlink()
    # Cargo filenames are date-stamped, but their mtime provides a common
    # current-run boundary across targets, including restored cache contents.
    earliest = min((phase["started_unix_ns"] for phase in phases), default=None)
    candidates = []
    available = 0
    if earliest is not None:
        for index, target in enumerate(targets):
            timings = target / "cargo-timings"
            if timings.is_symlink() or not timings.is_dir():
                continue
            with os.scandir(timings) as entries:
                for entry in entries:
                    # Exclude cargo-timing.html: Cargo also writes this alias
                    # of the newest date-stamped report, doubling upload bytes.
                    if not (entry.name.startswith("cargo-timing-") and entry.name.endswith(".html")
                            and entry.is_file(follow_symlinks=False)):
                        continue
                    info = entry.stat(follow_symlinks=False)
                    if info.st_mtime_ns < earliest:
                        continue
                    available += 1
                    item = (info.st_mtime_ns, index, entry.name, info.st_size, Path(entry.path))
                    if len(candidates) < MAX_REPORTS:
                        heapq.heappush(candidates, item)
                    else:
                        heapq.heappushpop(candidates, item)
    reports = []
    total = 0
    for _mtime, index, name, size, source in sorted(candidates, reverse=True):
        limit = min(MAX_REPORT_BYTES, MAX_TOTAL_REPORT_BYTES - total)
        if size > limit:
            continue
        # Bound reads as well as uploads if a report changes during collection.
        with source.open("rb") as stream:
            content = stream.read(limit + 1)
        if len(content) > limit:
            continue
        copied = output_path(destination, f"target-{index}-{name}")
        copied.write_bytes(content)
        reports.append({"source": str(source), "artifact_path": str(copied.relative_to(directory)),
                        "bytes": len(content)})
        total += len(content)
    return {"reports": reports, "available_current_reports": available,
            "retained_bytes": total, "max_reports": MAX_REPORTS,
            "max_report_bytes": MAX_REPORT_BYTES, "max_total_bytes": MAX_TOTAL_REPORT_BYTES}


def markdown(value):
    return str(value).replace("|", "\\|").replace("\n", " ").replace("\r", " ")


def summarize(targets, directory):
    current = prepare(directory)
    phases = read_phases(directory)
    sizes = [target_size(target, directory) for target in targets]
    timings = collect_timings(targets, directory, phases)
    summary = {"schema_version": 1, "context": current, "phases": phases, "targets": sizes,
               "cargo_timings": timings, "cache_archive_bytes": None,
               "cache_transfer_bytes": None}
    output_path(directory, "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    rows = ["### Maple Agent CI measurements", "", "| Phase | Target | Seconds | Exit |",
            "| --- | --- | ---: | ---: |"]
    rows.extend(f"| {markdown(row['phase'])} | {markdown(row['target'])} | "
                f"{row['elapsed_seconds']:.3f} | {row['exit_code']} |" for row in phases)
    rows.extend(["", "| Target | Logical bytes | Regular files | Scan errors |",
                 "| --- | ---: | ---: | ---: |"])
    rows.extend(f"| {markdown(row['target'])} | {row['logical_bytes']} | "
                f"{row['regular_files']} | {row['scan_errors']} |" for row in sizes)
    rows.extend(["", "Logical target bytes exclude symlinks and measurement artifacts. "
                 "Compressed cache archive and network transfer bytes are not measured.", "",
                 f"Retained {len(timings['reports'])} of {timings['available_current_reports']} "
                 f"current Cargo HTML timing reports ({timings['retained_bytes']} bytes). "
                 "Reports are bounded to 8 files, 16 MiB each, and 32 MiB total.", ""])
    rendered = "\n".join(rows)
    output_path(directory, "summary.md").write_text(rendered, encoding="utf-8")
    github_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_summary:
        with Path(github_summary).open("a", encoding="utf-8") as stream:
            stream.write(rendered)
    print(rendered)
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--phase")
    mode.add_argument("--summarize", nargs="+", type=Path, metavar="TARGET")
    parser.add_argument("--target", type=Path, default=Path("target"))
    parser.add_argument("--metrics-dir", type=Path, default=Path("target/ci-metrics"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.phase and not re.fullmatch(r"[A-Za-z0-9_.:-]{1,80}", args.phase):
        parser.error("phase must be 1-80 letters, digits, dots, colons, dashes, or underscores")
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if args.phase and not command:
        parser.error("--phase requires a command after --")
    if args.summarize and command:
        parser.error("--summarize does not execute a command")
    try:
        if args.phase:
            return run_phase(args.phase, args.target, command, args.metrics_dir)
        return summarize(args.summarize, args.metrics_dir)
    except (OSError, ValueError, KeyError, TypeError):
        # Do not print exception details: an executable argument/path could
        # contain a credential even though these workflows should never do so.
        print("CI measurement collection failed", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
