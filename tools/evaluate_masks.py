#!/usr/bin/env python3
"""Evaluate run products against V/I/Q/U0 truth planes (plan R1.5, section 2).

For each discovered input with a truth plane (same relative path under
--truth-dir; codes 0=V, 1=I, 2=Q, 3=U0), the newly excluded defined set M is
reconstructed from the products alone, independently of the Rust code:
pixels outside a crop rectangle (CLIPX0/CLIPY0, NAXIS1/2) plus pixels that are
undefined in the product but defined in the source. Kept images (symlinks)
have M empty. K is the retained defined set.

Metrics, with undefined denominators reported as null (never as perfect):
    removal recall        |M & I| / |I|
    removal precision     |M & I| / |M & (V | I)|
    valid loss            |M & V| / |V|
    residual contamination|K & I| / |K & (V | I)|
Q pixels are excluded; best/worst bounds count Q as V or I. Pooled (pixel-
weighted) and macro (per-image mean) values are given, with percentile 95%
intervals from a bootstrap over independent groups (default: the input's
parent directory, e.g. one night), never over correlated pixels.

Usage:
    python3 -I tools/evaluate_masks.py --summary RUN/summary.json \\
        --input-dir INPUT --truth-dir TRUTH [--group-depth 1] [--json out.json]
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from astropy.io import fits

V, I, Q, U0 = 0, 1, 2, 3
COUNT_KEYS = ("MI", "MV", "MQ", "KI", "KV", "KQ", "I", "V", "Q")


def read(path: Path) -> tuple[np.ndarray, fits.Header]:
    with fits.open(path, memmap=False) as hdus:
        return np.asarray(hdus[0].data), hdus[0].header.copy()


def undefined(data: np.ndarray, header: fits.Header) -> np.ndarray:
    if np.issubdtype(data.dtype, np.floating):
        return ~np.isfinite(data)
    if "BLANK" in header:
        return data == header["BLANK"]
    return np.zeros(data.shape, dtype=bool)


def excluded_mask(source_path: Path, product_path: Path | None) -> np.ndarray:
    """Defined source samples absent from the product as defined data."""
    source, source_header = read(source_path)
    src_undef = undefined(source, source_header)
    if product_path is None or product_path.is_symlink():
        return np.zeros(source.shape, dtype=bool)
    product, header = read(product_path)
    # MO_X0/MO_Y0 locate the product in the original frame; subtract the
    # source's own origin when the source is itself a cropped product.
    x0 = int(header.get("MO_X0", 0)) - int(source_header.get("MO_X0", 0))
    y0 = int(header.get("MO_Y0", 0)) - int(source_header.get("MO_Y0", 0))
    excluded = np.ones(source.shape, dtype=bool)
    h, w = product.shape
    excluded[y0 : y0 + h, x0 : x0 + w] = undefined(product, header)
    return excluded & ~src_undef


def counts(truth: np.ndarray, excluded: np.ndarray) -> dict:
    if truth.shape != excluded.shape:
        raise ValueError("truth plane shape differs from the source")
    defined = truth != U0
    kept = defined & ~excluded
    removed = defined & excluded
    out = {}
    for name, region in (("M", removed), ("K", kept)):
        for code, label in ((I, "I"), (V, "V"), (Q, "Q")):
            out[name + label] = int(np.count_nonzero(region & (truth == code)))
    for code, label in ((I, "I"), (V, "V"), (Q, "Q")):
        out[label] = int(np.count_nonzero(truth == code))
    return out


def ratio(numerator: float, denominator: float):
    return None if denominator == 0 else numerator / denominator


def metrics(c: dict) -> dict:
    """Point metrics with Q excluded, plus best/worst bounds over Q."""
    m = {
        "removal_recall": ratio(c["MI"], c["I"]),
        "removal_precision": ratio(c["MI"], c["MI"] + c["MV"]),
        "valid_loss": ratio(c["MV"], c["V"]),
        "residual_contamination": ratio(c["KI"], c["KI"] + c["KV"]),
        "uncertain_fraction": ratio(c["Q"], c["I"] + c["V"] + c["Q"]),
    }
    # Best case: removed Q were invalid and kept Q were valid; worst: reverse.
    m["valid_loss_best"] = ratio(c["MV"], c["V"] + c["KQ"])
    m["valid_loss_worst"] = ratio(c["MV"] + c["MQ"], c["V"] + c["Q"])
    m["removal_recall_best"] = ratio(c["MI"] + c["MQ"], c["I"] + c["MQ"])
    m["removal_recall_worst"] = ratio(c["MI"], c["I"] + c["KQ"])
    return m


def bootstrap(groups: dict[str, list[dict]], reps: int, seed: int) -> dict:
    names = sorted(groups)
    if len(names) < 2:
        return {"groups": len(names), "note": "fewer than two independent groups; no interval"}
    rng = np.random.default_rng(seed)
    samples: dict[str, list[float]] = {}
    for _ in range(reps):
        chosen = rng.choice(len(names), size=len(names), replace=True)
        total = dict.fromkeys(COUNT_KEYS, 0)
        for index in chosen:
            for record in groups[names[index]]:
                for key in COUNT_KEYS:
                    total[key] += record[key]
        for key, value in metrics(total).items():
            if value is not None:
                samples.setdefault(key, []).append(value)
    return {
        "groups": len(names),
        "replicates": reps,
        "interval_95": {k: [float(np.percentile(v, 2.5)), float(np.percentile(v, 97.5))] for k, v in samples.items()},
    }


def evaluate(summary_path: Path, input_dir: Path, truth_dir: Path, group_depth: int, reps: int, seed: int) -> dict:
    summary = json.loads(summary_path.read_text())
    per_image = []
    groups: dict[str, list[dict]] = {}
    for record in summary["files"]:
        relative = Path(record["input"])
        truth_path = truth_dir / relative
        if not truth_path.is_file():
            continue
        truth, _ = read(truth_path)
        output = Path(record["output_image"]) if record.get("output_image") else None
        excluded = excluded_mask(input_dir / relative, output)
        c = counts(truth.astype(np.uint8), excluded)
        group = str(relative.parent) if group_depth > 0 else "all"
        groups.setdefault(group, []).append(c)
        per_image.append({"input": str(relative), "action": record.get("action"), "group": group, **c, **metrics(c)})
    pooled = dict.fromkeys(COUNT_KEYS, 0)
    for row in per_image:
        for key in COUNT_KEYS:
            pooled[key] += row[key]
    macro = {}
    for key in ("removal_recall", "removal_precision", "valid_loss", "residual_contamination"):
        values = [row[key] for row in per_image if row[key] is not None]
        macro[key] = float(np.mean(values)) if values else None
        macro[key + "_worst_image"] = (max(values) if key in ("valid_loss", "residual_contamination") else min(values)) if values else None
    return {
        "images": len(per_image),
        "algorithm_version": summary.get("algorithm_version"),
        "pooled_counts": pooled,
        "pooled": metrics(pooled),
        "macro": macro,
        "bootstrap": bootstrap(groups, reps, seed),
        "per_image": per_image,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--summary", type=Path, required=True)
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--truth-dir", type=Path, required=True)
    parser.add_argument("--group-depth", type=int, default=1, help="0 = one group; 1 = parent directory")
    parser.add_argument("--bootstrap", type=int, default=2000)
    parser.add_argument("--seed", type=int, default=12345)
    parser.add_argument("--json", type=Path)
    args = parser.parse_args()
    result = evaluate(args.summary, args.input_dir, args.truth_dir, args.group_depth, args.bootstrap, args.seed)
    if args.json:
        args.json.write_text(json.dumps(result, indent=2))
    printable = {k: v for k, v in result.items() if k != "per_image"}
    print(json.dumps(printable, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
