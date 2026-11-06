#!/usr/bin/env python3
"""Compare plain KCP, TLS-over-KCP and TLS-over-TCP using prebuilt processes."""

import argparse
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import time

from middleware_compare import sha256, source_digest


def compare(args):
    binaries = {"benchmark": args.benchmark.resolve(), "server": args.server.resolve()}
    for path in (*binaries.values(), args.cert, args.key):
        if not path.is_file():
            raise ValueError(f"missing input: {path}")
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = {
        "platform": platform.platform(),
        "logical_cores": os.cpu_count(),
        "revision": args.revision,
        "source_sha256": source_digest(),
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
        "binaries": {name: {"path": str(path), "sha256": sha256(path)}
                     for name, path in binaries.items()},
        "tool_sources_sha256": {
            name: sha256(Path(__file__).resolve().parents[1] / name)
            for name in ("benches/kcp_tls_compare.py", "benches/benchmark.rs",
                         "benches/support/model1.rs", "examples/middleware_bench_server.rs",
                         "examples/support/allocation_counter.rs")
        },
        "conditions": {
            "model": 1, "network": "loopback, no injected impairment",
            "clients": args.clients, "payload_bytes": args.payload,
            "warmup_secs": args.warmup, "duration_secs": args.duration,
            "rounds": args.rounds, "server_runtime_workers": 4,
            "kcp": "fast, default parameters",
            "tls": "TLS 1.3; KCP+TLS disables resumption/0-RTT; fresh configs for TCP+TLS clients",
            "connection_setup": "whole connect_clients batch including any retries; before traffic window",
            "resources": "whole server lifetime, including startup/warmup/teardown; allocation requests, CPU, peak RSS",
            "logging": "no subscriber", "order": "rotated modes, serial cells, no builds",
        },
        "runs": [],
    }
    failed = False
    modes = ["kcp", "kcp-tls", "tcp-tls"]
    for round_index in range(args.rounds):
        offset = round_index % len(modes)
        for mode in modes[offset:] + modes[:offset]:
            for clients in args.clients:
                stem = f"round-{round_index + 1}-{mode}-{clients}"
                result_path = args.output / f"{stem}.json"
                stats_path = args.output / f"{stem}-server.json"
                log_path = args.output / f"{stem}.log"
                env = {key: value for key, value in os.environ.items()
                       if not key.startswith("LYNN_BENCH_")}
                env.update(
                    LYNN_BENCH_TRANSPORT="tcp" if mode == "tcp-tls" else "kcp",
                    LYNN_BENCH_KCP_MODE="fast", LYNN_MIDDLEWARE_MODE="empty",
                    LYNN_BENCH_SERVER=str(binaries["server"]),
                    LYNN_BENCH_SERVER_STATS=str(stats_path.resolve()),
                )
                if mode != "kcp":
                    env.update(LYNN_BENCH_TLS_CERT=str(args.cert.resolve()),
                               LYNN_BENCH_TLS_KEY=str(args.key.resolve()),
                               LYNN_BENCH_TLS_CA=str(args.cert.resolve()),
                               LYNN_BENCH_TLS_NAME="localhost")
                command = [str(binaries["benchmark"]), "--model", "1", "--clients", str(clients),
                           "--payload", str(args.payload), "--warmup", str(args.warmup),
                           "--duration", str(args.duration), "--json", str(result_path.resolve())]
                started = time.monotonic()
                timed_out = False
                launch_error = None
                with log_path.open("w") as log:
                    try:
                        process = subprocess.Popen(command, env=env, stdout=log, stderr=log,
                                                   start_new_session=True)
                    except OSError as error:
                        launch_error = str(error)
                        log.write(f"launch failed: {launch_error}\n")
                        code = 127
                    else:
                        try:
                            code = process.wait(timeout=args.timeout)
                        except subprocess.TimeoutExpired:
                            timed_out = True
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait()
                            code = 124
                record = {"round": round_index + 1, "mode": mode, "clients": clients,
                          "command": command, "exit_code": code, "timed_out": timed_out,
                          "elapsed_secs": time.monotonic() - started,
                          "log_file": log_path.name, "loadavg_after": os.getloadavg()}
                if launch_error is not None:
                    record["launch_error"] = launch_error
                failed |= code != 0
                try:
                    result = json.loads(result_path.read_text())["results"][0]
                    stats = json.loads(stats_path.read_text())
                    record.update(result=result, server=stats)
                    valid = (result["success"] and result["model"] == 1
                             and result["clients"] == clients and result["requests"] > 0
                             and result["requests"] == result["responses"]
                             and result["server_stop_succeeded"]
                             and len(result["client_outcomes"]) == clients
                             and all(client["exit"] == "window_complete"
                                     and client["requests"] == client["responses"] > 0
                                     for client in result["client_outcomes"])
                             and stats["handled_requests"] >= result["responses"])
                    record["valid"] = bool(valid)
                    failed |= not valid
                except (OSError, ValueError, TypeError, KeyError, IndexError) as error:
                    record["result_error"] = str(error)
                    failed = True
                manifest["runs"].append(record)
                (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
                print(f"{stem}: exit={code}", flush=True)
    return int(failed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("benchmark", "server", "cert", "key", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--clients", type=lambda value: [int(n) for n in value.split(",")], default=[1, 32])
    parser.add_argument("--payload", type=int, default=128)
    parser.add_argument("--warmup", type=int, default=1)
    parser.add_argument("--duration", type=int, default=5)
    parser.add_argument("--timeout", type=int, default=60)
    args = parser.parse_args()
    if min(args.rounds, args.warmup, args.duration, args.timeout, args.payload, *args.clients) <= 0:
        parser.error("rounds, clients, payload and durations must be positive")
    return compare(args)


if __name__ == "__main__":
    raise SystemExit(main())
