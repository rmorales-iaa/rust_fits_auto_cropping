#!/usr/bin/env python3
"""Production drift monitor over per-year run summaries (plan R5.2).

For each year it counts, among inputs the program decided on, how many had
each monitored event: every product action, an abstention reason, a protected
source inside the candidate area, an uncertain boundary band and a processing
error. Each year is tested against a reference rate from the *other* years
with an exact two-sided binomial test. The reference is the median of the
other years' rates when there are at least three of them, so one drifted year
cannot shift the baseline of the others; otherwise it is their pooled rate. Holm's step-down procedure controls the
family-wise error over all (year, event) tests. A flag means "review this
year for a domain change"; it never changes processing by itself.

The reference rate is treated as known, which is accurate when the other
years hold many more inputs than the tested year; with few years the test is
approximate. Years are labels supplied by the caller or taken from the
directory names of the yearly layout.
"""
from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

ACTIONS = ("keep", "mask", "crop", "crop_masked", "bbox_mask")


def events(record: dict) -> dict[str, bool]:
    reason = str(record.get("reason_code", ""))
    action = record.get("action")
    out = {f"action_{a}": action == a for a in ACTIONS}
    out["abstained"] = "abstain" in reason
    out["protected_source"] = (record.get("protected_source_pixels") or 0) > 0
    out["uncertain_band"] = (record.get("uncertain_boundary_pixels") or 0) > 0
    return out


EVENT_NAMES = tuple(events({}).keys()) + ("error",)


def year_counts(summary: Path) -> dict:
    files = json.loads(summary.read_text()).get("files", [])
    counts = {name: 0 for name in EVENT_NAMES}
    decided = 0
    for record in files:
        status = record.get("status")
        if status == "error":
            counts["error"] += 1
        if status in ("ignored", "error") or "action" not in record:
            continue
        decided += 1
        for name, hit in events(record).items():
            counts[name] += int(hit)
    return {"inputs": len(files), "decided": decided, "counts": counts}


def median_of(values: list[float]) -> float:
    ordered = sorted(values)
    mid = len(ordered) // 2
    return ordered[mid] if len(ordered) % 2 else 0.5 * (ordered[mid - 1] + ordered[mid])


def binomial_two_sided(k: int, n: int, p: float) -> float:
    """Exact two-sided binomial p-value (sum of outcomes no more likely than k)."""
    if n == 0:
        return 1.0
    if p <= 0.0:
        return 1.0 if k == 0 else 0.0
    if p >= 1.0:
        return 1.0 if k == n else 0.0
    log_p, log_q = math.log(p), math.log1p(-p)
    lg_n = math.lgamma(n + 1)

    def log_pmf(i: int) -> float:
        return lg_n - math.lgamma(i + 1) - math.lgamma(n - i + 1) + i * log_p + (n - i) * log_q

    threshold = log_pmf(k) + 1e-7
    total = sum(math.exp(lp) for lp in (log_pmf(i) for i in range(n + 1)) if lp <= threshold)
    return min(1.0, total)


def holm(pvalues: list[float], alpha: float) -> list[bool]:
    order = sorted(range(len(pvalues)), key=lambda i: pvalues[i])
    reject = [False] * len(pvalues)
    m = len(pvalues)
    for rank, i in enumerate(order):
        if pvalues[i] > alpha / (m - rank):
            break
        reject[i] = True
    return reject


def monitor(runs: dict[str, Path], alpha: float) -> dict:
    if len(runs) < 2:
        raise ValueError("need at least two years to compare")
    years = {year: year_counts(path) for year, path in sorted(runs.items())}
    tests = []
    for year, info in years.items():
        for name in EVENT_NAMES:
            base = "inputs" if name == "error" else "decided"
            n, k = info[base], info["counts"][name]
            others = [o for y, o in years.items() if y != year and o[base] > 0]
            if n == 0 or not others:
                continue
            if len(others) >= 3:
                p0 = float(median_of([o["counts"][name] / o[base] for o in others]))
            else:
                p0 = sum(o["counts"][name] for o in others) / sum(o[base] for o in others)
            tests.append(
                {
                    "year": year,
                    "event": name,
                    "k": k,
                    "n": n,
                    "rate": k / n,
                    "reference_rate": p0,
                    "p_value": binomial_two_sided(k, n, p0),
                }
            )
    for test, flag in zip(tests, holm([t["p_value"] for t in tests], alpha)):
        test["flagged"] = flag
    return {"kind": "drift_monitor", "alpha": alpha, "years": years, "tests": tests}


def discover(root: Path) -> dict[str, Path]:
    """Yearly layout: ROOT/<year>/science_calibrated/summary.json (or ROOT/<year>/summary.json)."""
    runs = {}
    for child in sorted(p for p in root.iterdir() if p.is_dir() and not p.is_symlink()):
        for candidate in (child / "science_calibrated" / "summary.json", child / "summary.json"):
            if candidate.is_file():
                runs[child.name] = candidate
                break
    return runs


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--run", action="append", default=[], help="YEAR=PATH/summary.json, repeatable")
    parser.add_argument("--root", type=Path, help="discover ROOT/<year>/science_calibrated/summary.json")
    parser.add_argument("--alpha", type=float, default=0.05, help="family-wise error rate (Holm)")
    parser.add_argument("--json", type=Path)
    args = parser.parse_args(argv)
    try:
        if not 0.0 < args.alpha < 1.0:
            raise ValueError("--alpha must lie in (0, 1)")
        runs = discover(args.root) if args.root else {}
        for item in args.run:
            year, _, path = item.partition("=")
            if not year or not path:
                raise ValueError(f"--run expects YEAR=PATH: {item}")
            runs[year] = Path(path)
        result = monitor(runs, args.alpha)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    if args.json:
        args.json.write_text(json.dumps(result, indent=2) + "\n")
    for year, info in result["years"].items():
        rates = ", ".join(
            f"{a} {info['counts']['action_' + a] / info['decided']:.3f}" for a in ACTIONS if info["decided"]
        )
        print(f"{year}: {info['inputs']} inputs, {info['decided']} decided; {rates}")
    flagged = [t for t in result["tests"] if t["flagged"]]
    for t in flagged:
        print(f"FLAG {t['year']} {t['event']}: {t['k']}/{t['n']} = {t['rate']:.3f} vs other years {t['reference_rate']:.3f} (p = {t['p_value']:.2e})")
    print(f"{len(flagged)} of {len(result['tests'])} tests flagged at family-wise alpha {args.alpha}")
    return 1 if flagged else 0


if __name__ == "__main__":
    raise SystemExit(main())
