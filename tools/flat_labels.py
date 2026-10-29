#!/usr/bin/env python3
"""Flat-derived V/I/Q evaluation labels (plan R1.3; decisions D3 and D4).

A matched master flat F measures relative illumination independently of the
detector, which never sees it. With eta = F / P, where P is the robust level
of illuminated flat pixels, each science pixel is labelled

    0 = V   usable          eta >= eta_v   (default 0.50)
    1 = I   invalid         eta <  eta_i   (default 0.20), or flat <= 0 / non-finite
    2 = Q   uncertain       eta_i <= eta < eta_v
    3 = U0  science sample already undefined (NaN or BLANK)

P is the median of finite flat pixels with F >= 0.5 * median(finite F > 0),
which excludes unilluminated regions from the normalisation.

Labels are evaluation-only (decision D3). A flat cannot label pointing-
dependent occultation, scattered light, overscan or readout artefacts, and
dome flats can differ from sky illumination; report those limits with any
metric. Matching a flat to a science frame (night, filter, binning) uses the
calibration pipeline's provenance and is the caller's responsibility; this
tool only checks that the array shapes agree.

Usage:
    python3 -I tools/flat_labels.py --flat FLAT --science SCI --output TRUTH
    python3 -I tools/flat_labels.py --pairs pairs.csv     # science,flat,output
"""
from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path

import numpy as np
from astropy.io import fits

V, I, Q, U0 = 0, 1, 2, 3


def illumination(flat: np.ndarray) -> np.ndarray:
    """Relative illumination eta; NaN where the flat is unusable."""
    flat = np.asarray(flat, dtype=np.float64)
    positive = flat[np.isfinite(flat) & (flat > 0)]
    if positive.size == 0:
        raise ValueError("flat has no finite positive pixels")
    first = np.median(positive)
    illuminated = positive[positive >= 0.5 * first]
    level = np.median(illuminated)
    eta = flat / level
    eta[~np.isfinite(flat) | (flat <= 0)] = np.nan
    return eta


def undefined_science(path: Path) -> np.ndarray:
    with fits.open(path, memmap=False) as hdus:
        data = np.asarray(hdus[0].data)
        header = hdus[0].header
    if np.issubdtype(data.dtype, np.floating):
        return ~np.isfinite(data)
    if "BLANK" in header:
        return data == header["BLANK"]
    return np.zeros(data.shape, dtype=bool)


def labels(eta: np.ndarray, eta_i: float, eta_v: float, undefined: np.ndarray | None = None) -> np.ndarray:
    if not 0.0 <= eta_i < eta_v <= 1.0:
        raise ValueError("require 0 <= eta_i < eta_v <= 1")
    truth = np.full(eta.shape, Q, dtype=np.uint8)
    with np.errstate(invalid="ignore"):
        truth[eta >= eta_v] = V
        truth[eta < eta_i] = I
    truth[~np.isfinite(eta)] = I
    if undefined is not None:
        if undefined.shape != eta.shape:
            raise ValueError(f"science shape {undefined.shape} differs from flat {eta.shape}")
        truth[undefined] = U0
    return truth


def write_labels(flat_path: Path, science_path: Path | None, output: Path, eta_i: float, eta_v: float) -> dict:
    with fits.open(flat_path, memmap=False) as hdus:
        flat = np.asarray(hdus[0].data, dtype=np.float64)
    undefined = undefined_science(science_path) if science_path else None
    truth = labels(illumination(flat), eta_i, eta_v, undefined)
    hdu = fits.PrimaryHDU(truth)
    hdu.header["TSCHEME"] = ("VIQU0", "0=V 1=I 2=Q 3=U0; evaluation only")
    hdu.header["ETA_I"] = (eta_i, "invalid below this relative illumination")
    hdu.header["ETA_V"] = (eta_v, "valid at or above this relative illumination")
    hdu.header["LABELSRC"] = ("FLAT", "labels derived from a matched master flat")
    output.parent.mkdir(parents=True, exist_ok=True)
    hdu.writeto(output, overwrite=True)
    counts = np.bincount(truth.ravel(), minlength=4)
    return {"output": str(output), "V": int(counts[V]), "I": int(counts[I]), "Q": int(counts[Q]), "U0": int(counts[U0])}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--flat", type=Path)
    parser.add_argument("--science", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--pairs", type=Path, help="CSV with columns science,flat,output")
    parser.add_argument("--eta-i", type=float, default=0.20)
    parser.add_argument("--eta-v", type=float, default=0.50)
    args = parser.parse_args()
    jobs = []
    if args.pairs:
        with args.pairs.open(newline="") as stream:
            for row in csv.DictReader(stream):
                jobs.append((Path(row["flat"]), Path(row["science"]) if row.get("science") else None, Path(row["output"])))
    elif args.flat and args.output:
        jobs.append((args.flat, args.science, args.output))
    else:
        parser.error("give --flat and --output, or --pairs")
    for flat, science, output in jobs:
        print(write_labels(flat, science, output, args.eta_i, args.eta_v))
    return 0


if __name__ == "__main__":
    sys.exit(main())
