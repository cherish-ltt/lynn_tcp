#!/usr/bin/env python3
"""Compare prebuilt independent servers serially; retain every round and exit code."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time


def prepare_baseline(destination):
    root = Path(__file__).resolve().parents[1]
    source = (root / "examples/middleware_bench_server.rs").read_text()
    before, rest = source.split("// BEGIN MIDDLEWARE\n", 1)
    _, after = rest.split("// END MIDDLEWARE\n", 1)
    replacement = """mod middleware {
    use super::*;
    pub(super) fn configure<'a>(server: LynnServer<'a>, mode: &str) -> LynnServer<'a> {
        assert_eq!(mode, "empty");
        server
    }
}
"""
    target = destination / "examples"
    (target / "support").mkdir(parents=True, exist_ok=True)
    (target / "middleware_bench_server.rs").write_text(before + replacement + after)
    shutil.copyfile(
        root / "examples/support/allocation_counter.rs",
        target / "support/allocation_counter.rs",
    )


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_digest():
    root = Path(__file__).resolve().parents[1]
    paths = sorted((root / "src").rglob("*.rs")) + [root / "Cargo.toml", root / "Cargo.lock"]
    digest = hashlib.sha256()
    for path in paths:
        digest.update(str(path.relative_to(root)).encode() + b"\0" + path.read_bytes())
    return digest.hexdigest()


def compare(args):
    binaries = {
        "benchmark": args.benchmark.resolve(),
        "baseline": args.baseline_server.resolve(),
        "current": args.server.resolve(),
    }
    for binary in binaries.values():
        if not binary.is_file():
            raise ValueError(f"missing prebuilt executable: {binary}")
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = {
        "platform": platform.platform(),
        "logical_cores": os.cpu_count(),
        "unix_timestamp": int(time.time()),
        "baseline_revision": args.baseline_revision,
        "current_revision": args.current_revision,
        "current_source_sha256": source_digest(),
        "tool_sources_sha256": {
            name: sha256(Path(__file__).resolve().parents[1] / name)
            for name in (
                "benches/middleware_compare.py",
                "benches/benchmark.rs",
                "benches/support/model1.rs",
                "examples/middleware_bench_server.rs",
                "examples/support/allocation_counter.rs",
            )
        },
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
        "binaries": {
            name: {"path": str(path), "sha256": sha256(path)}
            for name, path in binaries.items()
        },
        "conditions": {
            "model": 1,
            "transport": "tcp loopback",
            "clients": args.clients,
            "payload_bytes": args.payload,
            "warmup_secs": args.warmup,
            "duration_secs": args.duration,
            "server_runtime_workers": 4,
            "allocation_scope": "whole server lifetime including startup, warmup and teardown",
            "allocation_counter": "System with relaxed atomics; alloc/alloc_zeroed/realloc requests",
            "logging": "no subscriber",
        },
        "runs": [],
    }
    failed = False
    modes = ["baseline", "empty", "pass", "composed"]
    for round_index in range(args.rounds):
        # Rotate order to reduce consistent temperature/order bias.
        for mode in modes[round_index % 4 :] + modes[: round_index % 4]:
            for clients in args.clients:
                stem = f"round-{round_index + 1}-{mode}-{clients}"
                result_file = args.output / f"{stem}.json"
                stats_file = args.output / f"{stem}-server.json"
                env = os.environ.copy()
                env.pop("LYNN_BENCH_LOGS", None)
                env.update(
                    LYNN_BENCH_TRANSPORT="tcp",
                    LYNN_BENCH_SERVER=str(
                        binaries["baseline" if mode == "baseline" else "current"]
                    ),
                    LYNN_MIDDLEWARE_MODE="empty" if mode == "baseline" else mode,
                    LYNN_BENCH_SERVER_STATS=str(stats_file.resolve()),
                )
                command = [
                    str(binaries["benchmark"]), "--model", "1",
                    "--clients", str(clients), "--payload", str(args.payload),
                    "--warmup", str(args.warmup), "--duration", str(args.duration),
                    "--json", str(result_file.resolve()),
                ]
                started = time.monotonic()
                with (args.output / f"{stem}.log").open("w") as log:
                    process = subprocess.run(command, env=env, stdout=log, stderr=log, check=False)
                run = {
                    "round": round_index + 1,
                    "mode": mode,
                    "clients": clients,
                    "exit_code": process.returncode,
                    "elapsed_secs": time.monotonic() - started,
                    "result_file": result_file.name if result_file.exists() else None,
                    "server_stats_file": stats_file.name if stats_file.exists() else None,
                    "log_file": f"{stem}.log",
                }
                failed |= process.returncode != 0
                try:
                    result = json.loads(result_file.read_text())["results"][0]
                    outcomes = result["client_outcomes"]
                    run["requests_equal_responses"] = result["requests"] == result["responses"]
                    run["measured_requests_positive"] = result["requests"] > 0
                    run["benchmark_success"] = result["success"]
                    run["all_clients_completed"] = len(outcomes) == clients and all(
                        client["exit"] == "window_complete"
                        and client["requests"] > 0
                        and client["requests"] == client["responses"]
                        for client in outcomes
                    )
                    run["server_stop_succeeded"] = result["server_stop_succeeded"]
                    failed |= not all(run[key] for key in (
                        "requests_equal_responses", "measured_requests_positive",
                        "benchmark_success", "all_clients_completed", "server_stop_succeeded"
                    ))
                except (OSError, ValueError, KeyError, IndexError) as error:
                    run["result_error"] = str(error)
                    failed = True
                try:
                    stats = json.loads(stats_file.read_text())
                    run["handled_requests_positive"] = stats["handled_requests"] > 0
                    failed |= not run["handled_requests_positive"]
                except (OSError, ValueError, KeyError) as error:
                    run["server_stats_error"] = str(error)
                    failed = True
                manifest["runs"].append(run)
                (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
                print(f"{stem}: exit={process.returncode}", flush=True)
    return int(failed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare-baseline", type=Path)
    parser.add_argument("--benchmark", type=Path)
    parser.add_argument("--server", type=Path)
    parser.add_argument("--baseline-server", type=Path)
    parser.add_argument("--baseline-revision")
    parser.add_argument("--current-revision")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--clients", type=lambda value: [int(n) for n in value.split(",")], default=[1, 64])
    parser.add_argument("--warmup", type=int, default=2)
    parser.add_argument("--duration", type=int, default=5)
    parser.add_argument("--payload", type=int, default=128)
    args = parser.parse_args()
    if args.prepare_baseline:
        prepare_baseline(args.prepare_baseline)
        return 0
    for name in ("benchmark", "server", "baseline_server", "baseline_revision", "current_revision", "output"):
        if getattr(args, name) is None:
            parser.error(f"--{name.replace('_', '-')} is required for measurement")
    if min(args.rounds, args.warmup, args.duration, args.payload, *args.clients) <= 0:
        parser.error("rounds, clients, warmup, duration and payload must be positive")
    return compare(args)


if __name__ == "__main__":
    sys.exit(main())
