#!/usr/bin/env python3
"""Run randomized, interleaved release CLI trials and summarize time/RSS.

This is an engineering benchmark, not a scientific-accuracy evaluator. It keeps
all trial products, process output, GNU time records, and the JSON summary under
one caller-selected output root.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import platform
import random
import shutil
import statistics
import subprocess
from datetime import datetime, timezone
from pathlib import Path

SIGNATURE_FIELDS = (
    "input", "status", "reason_code", "source_sha256", "masked_pixels",
    "crop_shape", "crop_origin_x", "crop_origin_y", "crop_width",
    "crop_height", "crop_center_x", "crop_center_y", "crop_radius",
    "image_area_retained", "artifact_pixels_removed", "unmasked_pixels_sacrificed",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def decision_digest(csv_path: Path) -> str:
    rows = []
    with csv_path.open(newline="", encoding="utf-8") as stream:
        for row in csv.DictReader(stream):
            rows.append({key: row.get(key, "") for key in SIGNATURE_FIELDS})
    canonical = json.dumps(rows, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--heuristics", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--warmup-runs", type=int, default=1)
    parser.add_argument("--workers", type=int, nargs="+", default=[1, 4])
    parser.add_argument("--memory-budget-mib", type=int, default=2048)
    parser.add_argument("--seed", type=int, default=20261027)
    args = parser.parse_args()
    for path in (args.binary, args.input_dir, args.config, args.heuristics):
        if not path.exists():
            parser.error(f"required path does not exist: {path}")
    if args.runs < 1 or args.warmup_runs < 0 or not args.workers or any(worker < 1 for worker in args.workers):
        parser.error("runs/worker counts must be positive and warm-up runs nonnegative")
    output = args.output_dir.resolve()
    if output.exists() and any(output.iterdir()):
        parser.error("benchmark output directory must be empty or not exist")
    if shutil.which("/usr/bin/time") is None:
        parser.error("GNU /usr/bin/time is required for peak RSS measurements")
    output.mkdir(parents=True, exist_ok=True)
    rng = random.Random(args.seed)
    warmups = [(worker, repeat + 1, True) for repeat in range(args.warmup_runs) for worker in args.workers]
    measured = [(worker, repeat + 1, False) for repeat in range(args.runs) for worker in args.workers]
    rng.shuffle(warmups)
    rng.shuffle(measured)
    schedule = warmups + measured
    results = []
    for index, (workers, repeat, is_warmup) in enumerate(schedule, 1):
        group = "warmup" if is_warmup else "trial"
        stdout_path = output / f"{group}-{index:02d}-w{workers}-r{repeat}.log"
        time_path = output / f"{group}-{index:02d}-w{workers}-r{repeat}.time"
        command = [
            "/usr/bin/time", "-f", "elapsed_seconds=%e\nmax_rss_kib=%M",
            "-o", str(time_path), str(args.binary.resolve()),
            "--input-dir", str(args.input_dir.resolve()),
            "--output-dir", str(output), "--config", str(args.config.resolve()),
            "--heuristics", str(args.heuristics.resolve()), "--recursive",
            "--mode", "crop", "--threads", str(workers),
            "--memory-budget-mib", str(args.memory_budget_mib),
        ]
        completed = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        stdout_path.write_text(completed.stdout, encoding="utf-8")
        run_ids = [line.split()[1].rstrip(":") for line in completed.stdout.splitlines() if line.startswith("Run ")]
        if not run_ids:
            raise RuntimeError(f"trial {index} emitted no run id; inspect {stdout_path}")
        run_dir = output / "runs" / run_ids[-1]
        summary_path = run_dir / "summary.json"
        csv_path = run_dir / "per-image.csv"
        if not summary_path.is_file() or not csv_path.is_file():
            raise RuntimeError(f"trial {index} did not publish its summary/report: {run_dir}")
        time_values = {}
        for line in time_path.read_text(encoding="utf-8").splitlines():
            if "=" not in line:
                continue
            key, value = line.split("=", 1)
            time_values[key] = float(value)
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
        results.append({
            "trial": index, "repeat": repeat, "workers": workers, "warmup": is_warmup,
            "run_id": run_ids[-1], "exit_code": completed.returncode,
            "wall_seconds": time_values["elapsed_seconds"],
            "peak_rss_kib": int(time_values["max_rss_kib"]),
            "summary_counts": {key: summary.get(key) for key in ("total", "clipped", "kept", "ignored", "errors")},
            "decision_sha256": decision_digest(csv_path),
            "stdout_log": str(stdout_path), "time_log": str(time_path),
        })
    signatures = {row["decision_sha256"] for row in results}
    if len(signatures) != 1:
        raise RuntimeError("worker/repeat decision signatures differ; inspect per-image reports")
    measured_results = [row for row in results if not row["warmup"]]
    warmup_results = [row for row in results if row["warmup"]]
    groups = {}
    for worker in sorted(set(args.workers)):
        rows = [row for row in measured_results if row["workers"] == worker]
        groups[str(worker)] = {
            "trials": len(rows),
            "median_wall_seconds": statistics.median(row["wall_seconds"] for row in rows),
            "median_peak_rss_kib": statistics.median(row["peak_rss_kib"] for row in rows),
            "wall_seconds": [row["wall_seconds"] for row in rows],
            "peak_rss_kib": [row["peak_rss_kib"] for row in rows],
        }
    report = {
        "schema_version": 1,
        "engineering_benchmark_only": True,
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "binary": str(args.binary.resolve()), "binary_sha256": sha256(args.binary.resolve()),
        "input_dir": str(args.input_dir.resolve()), "output_dir": str(output),
        "config_sha256": sha256(args.config.resolve()),
        "heuristics_sha256": sha256(args.heuristics.resolve()),
        "runs_per_worker": args.runs, "warmup_runs_per_worker": args.warmup_runs,
        "workers": args.workers, "memory_budget_mib": args.memory_budget_mib, "random_seed": args.seed,
        "warmup_trials": warmup_results,
        "schedule": [{"trial": row["trial"], "workers": row["workers"], "repeat": row["repeat"], "warmup": row["warmup"]} for row in results],
        "hardware": {"platform": platform.platform(), "processor": platform.processor(), "logical_cpus": __import__("os").cpu_count()},
        "decision_signatures_identical": len(signatures) == 1,
        "decision_sha256": next(iter(signatures)), "by_worker": groups, "trials": results,
    }
    (output / "benchmark-summary.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(groups, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
