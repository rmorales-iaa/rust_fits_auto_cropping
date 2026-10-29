#!/usr/bin/env python3
"""Shadow comparison and enriched labelling design for two detectors (plan R5.0).

Two subcommands:

``design``
    Read two run summaries over the same inputs (A = candidate, B = reference;
    dry-run summaries are enough). Put each input into one stratum:
    ``agree_keep`` (neither proposes a mask), ``agree_act`` (both propose
    similar masks), ``extent`` (both propose, pixel counts differ by more
    than ``--extent-ratio``) or ``disagree`` (exactly one proposes). Draw a
    seeded stratified simple random sample without replacement and record each
    sampled input's inclusion probability pi_h = n_h / N_h. Oversampling the
    disagreement strata increases power for detector differences; the recorded
    pi_h keeps population estimates unbiased.

``estimate``
    Combine the design with ``tools/evaluate_masks.py`` JSON for each detector
    on the labelled sample. Horvitz-Thompson totals and linearised ratio
    estimators give population valid loss, removal recall and removal
    precision for A and B and for the paired difference A - B. Variances use
    the stratified design with finite-population correction. The rollback
    check of plan section 0R.1 compares A's one-sided 95% upper bound of
    valid loss with B's point estimate.

The estimates target the finite population summarised by the runs (for
example one archive year). They are design-based: they do not assume frames
are independent. Never report metrics from the disagreement strata alone.
"""
from __future__ import annotations

import argparse
import json
import math
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

STRATA = ("agree_keep", "agree_act", "extent", "disagree")
Z95_ONE_SIDED = 1.6448536269514722
Z95_TWO_SIDED = 1.959963984540054


def load_files(summary: Path) -> dict[str, dict]:
    data = json.loads(summary.read_text())
    return {record["input"]: record for record in data.get("files", [])}


def proposed(record: dict) -> int:
    value = record.get("proposed_mask_pixels")
    return int(value) if isinstance(value, (int, float)) and value > 0 else 0


def stratum(a: dict, b: dict, extent_ratio: float) -> str:
    pa, pb = proposed(a), proposed(b)
    if pa == 0 and pb == 0:
        return "agree_keep"
    if pa == 0 or pb == 0:
        return "disagree"
    return "extent" if max(pa, pb) > extent_ratio * min(pa, pb) else "agree_act"


def eligible(record: dict) -> bool:
    """Only inputs the program decided on (not ignored, not errors)."""
    return record.get("status") not in ("ignored", "error") and "proposed_mask_pixels" in record


def design(summary_a: Path, summary_b: Path, sizes: dict[str, int], seed: int, extent_ratio: float) -> dict:
    a, b = load_files(summary_a), load_files(summary_b)
    if set(a) != set(b):
        raise ValueError("the two summaries cover different inputs")
    population: dict[str, list[str]] = {h: [] for h in STRATA}
    excluded = 0
    for name in sorted(a):
        if not (eligible(a[name]) and eligible(b[name])):
            excluded += 1
            continue
        population[stratum(a[name], b[name], extent_ratio)].append(name)
    rng = np.random.default_rng(seed)
    strata = {}
    sample = []
    for h in STRATA:
        members = population[h]
        n_h = min(sizes.get(h, 0), len(members))
        chosen = sorted(rng.choice(len(members), size=n_h, replace=False).tolist()) if n_h else []
        pi = n_h / len(members) if members else 0.0
        strata[h] = {"N": len(members), "n": n_h, "pi": pi}
        sample += [{"input": members[i], "stratum": h, "pi": pi} for i in chosen]
    return {
        "kind": "shadow_design",
        "summary_a": str(summary_a),
        "summary_b": str(summary_b),
        "seed": seed,
        "extent_ratio": extent_ratio,
        "excluded_inputs": excluded,
        "strata": strata,
        "sample": sample,
    }


def _per_image(evaluation: Path) -> dict[str, dict]:
    return {record["input"]: record for record in json.loads(evaluation.read_text())["per_image"]}


def _stratified_variance(strata: dict, values: dict[str, list[float]]) -> float:
    """Var of the HT total of z under stratified SRS without replacement."""
    variance = 0.0
    for h, info in strata.items():
        z = values.get(h, [])
        n, big_n = len(z), info["N"]
        if n < 2:
            if big_n > n:
                return math.nan  # unidentifiable: a stratum with < 2 labels and unsampled members
            continue
        variance += big_n * big_n * (1.0 - n / big_n) * float(np.var(z, ddof=1)) / n
    return variance


METRICS = {
    # name: (numerator count, denominator counts)
    "valid_loss": ("MV", ("MV", "KV")),
    "removal_recall": ("MI", ("MI", "KI")),
    "removal_precision": ("MI", ("MI", "MV")),
}


def _ratio(strata, rows, num, den):
    """HT ratio estimate and the linearised residual per sampled unit."""
    total = defaultdict(float)
    for h, row in rows:
        weight = strata[h]["N"] / strata[h]["n"]
        total["num"] += weight * row[num]
        total["den"] += weight * sum(row[k] for k in den)
    if total["den"] <= 0:
        return math.nan, {}
    r = total["num"] / total["den"]
    z = defaultdict(list)
    for h, row in rows:
        z[h].append((row[num] - r * sum(row[k] for k in den)) / total["den"])
    return r, z


def estimate(design_doc: dict, evaluation_a: Path, evaluation_b: Path) -> dict:
    strata = {h: v for h, v in design_doc["strata"].items() if v["N"] > 0}
    for h, v in strata.items():
        if v["n"] < min(2, v["N"]):
            raise ValueError(f"stratum {h} has {v['N']} inputs but {v['n']} labelled; need at least min(2, N)")
    eval_a, eval_b = _per_image(evaluation_a), _per_image(evaluation_b)
    rows_a, rows_b, missing = [], [], []
    for unit in design_doc["sample"]:
        name = unit["input"]
        if name not in eval_a or name not in eval_b:
            missing.append(name)
            continue
        rows_a.append((unit["stratum"], eval_a[name]))
        rows_b.append((unit["stratum"], eval_b[name]))
    if missing:
        raise ValueError(f"{len(missing)} sampled inputs lack evaluations, e.g. {missing[0]}")
    result = {"kind": "shadow_estimate", "strata": strata, "metrics": {}}
    for metric, (num, den) in METRICS.items():
        ra, za = _ratio(strata, rows_a, num, den)
        rb, zb = _ratio(strata, rows_b, num, den)
        se_a = math.sqrt(_stratified_variance(strata, za)) if za else math.nan
        se_b = math.sqrt(_stratified_variance(strata, zb)) if zb else math.nan
        zd = {h: [x - y for x, y in zip(za.get(h, []), zb.get(h, []))] for h in strata}
        se_d = math.sqrt(_stratified_variance(strata, zd)) if za and zb else math.nan
        result["metrics"][metric] = {
            "a": ra,
            "a_se": se_a,
            "a_upper95": ra + Z95_ONE_SIDED * se_a,
            "a_lower95": ra - Z95_ONE_SIDED * se_a,
            "b": rb,
            "b_se": se_b,
            "difference": ra - rb,
            "difference_ci95": [ra - rb - Z95_TWO_SIDED * se_d, ra - rb + Z95_TWO_SIDED * se_d],
        }
    loss = result["metrics"]["valid_loss"]
    result["rollback_valid_loss_trigger"] = (
        None if math.isnan(loss["a_upper95"]) else bool(loss["a_upper95"] > loss["b"])
    )
    labelled = sum(v["n"] for v in strata.values())
    result["note"] = (
        "Normal-approximation bounds; treat as indicative when any stratum has fewer than 30 labels."
        if any(0 < v["n"] < 30 for v in strata.values()) or labelled < 60
        else "Design-based bounds for the summarised finite population."
    )
    return result


def parse_sizes(items: list[str]) -> dict[str, int]:
    sizes = {}
    for item in items:
        name, _, value = item.partition("=")
        if name not in STRATA or not value.isdigit():
            raise ValueError(f"--n expects STRATUM=COUNT with STRATUM in {', '.join(STRATA)}: {item}")
        sizes[name] = int(value)
    return sizes


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    d = sub.add_parser("design", help="stratify two runs and draw the labelling sample")
    d.add_argument("--summary-a", type=Path, required=True, help="candidate detector (e.g. background_v2)")
    d.add_argument("--summary-b", type=Path, required=True, help="reference detector (e.g. tile_v1)")
    d.add_argument("--n", action="append", default=[], help="STRATUM=COUNT, repeatable")
    d.add_argument("--seed", type=int, required=True)
    d.add_argument("--extent-ratio", type=float, default=2.0)
    d.add_argument("--json", type=Path, required=True)
    e = sub.add_parser("estimate", help="population estimates from labelled sample evaluations")
    e.add_argument("--design", type=Path, required=True)
    e.add_argument("--evaluation-a", type=Path, required=True, help="evaluate_masks.py JSON for detector A")
    e.add_argument("--evaluation-b", type=Path, required=True, help="evaluate_masks.py JSON for detector B")
    e.add_argument("--json", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "design":
            if args.extent_ratio < 1.0:
                raise ValueError("--extent-ratio must be >= 1")
            doc = design(args.summary_a, args.summary_b, parse_sizes(args.n), args.seed, args.extent_ratio)
            args.json.write_text(json.dumps(doc, indent=2) + "\n")
            for h, v in doc["strata"].items():
                print(f"{h:<11} N={v['N']:<7} n={v['n']:<5} pi={v['pi']:.4f}")
            print(f"sample of {len(doc['sample'])} written to {args.json}")
        else:
            doc = estimate(json.loads(args.design.read_text()), args.evaluation_a, args.evaluation_b)
            if args.json:
                args.json.write_text(json.dumps(doc, indent=2) + "\n")
            for metric, m in doc["metrics"].items():
                lo, hi = m["difference_ci95"]
                print(f"{metric:<18} A {m['a']:.5f} (se {m['a_se']:.5f})  B {m['b']:.5f}  A-B {m['difference']:+.5f} [{lo:+.5f}, {hi:+.5f}]")
            print(f"rollback trigger (A valid-loss upper 95% > B point estimate): {doc['rollback_valid_loss_trigger']}")
            print(doc["note"])
    except (OSError, ValueError, KeyError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
