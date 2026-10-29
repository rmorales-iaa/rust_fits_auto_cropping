#!/usr/bin/env python3
"""Seeded synthetic FITS corpus with exact truth planes (plan R1.4).

Each fixture is a single-HDU 2D image plus a uint8 truth plane written under
``truth/`` using the V/I/Q/U0 convention of the improvement plan:

    0 = V   usable science (relative illumination eta >= eta_v)
    1 = I   instrumentally invalid border (eta < eta_i, or artifact plateau)
    2 = Q   uncertain (eta_i <= eta < eta_v)
    3 = U0  originally undefined sample (NaN / BLANK)

Defaults eta_i = 0.20 and eta_v = 0.50 follow user decision D4. Noise uses a
pixel-level mean-variance model sigma^2 = read^2 + signal/gain; calibrated
families divide by eta to reproduce flat-field noise amplification. These are
method fixtures only: synthetic success never substitutes for real labels.

Run as an isolated script, e.g. ``python3 -I tools/make_synthetic_corpus.py OUT``.
"""
from __future__ import annotations

import argparse
import json
import zlib
from pathlib import Path

import numpy as np
from astropy.io import fits

V, I, Q, U0 = 0, 1, 2, 3
GENERATOR_VERSION = "synthetic-corpus-v2-physical-noise"


def labels_from_eta(eta: np.ndarray, eta_i: float, eta_v: float) -> np.ndarray:
    truth = np.full(eta.shape, Q, dtype=np.uint8)
    truth[eta >= eta_v] = V
    truth[eta < eta_i] = I
    return truth


def sky(rng: np.random.Generator, shape, level=1000.0, read=5.0, gain=2.0) -> np.ndarray:
    signal = np.full(shape, level)
    sigma = np.sqrt(read**2 + signal / gain)
    return signal + rng.normal(0.0, 1.0, shape) * sigma


def illuminated(rng: np.random.Generator, eta: np.ndarray, level=1000.0, read=5.0, gain=2.0) -> np.ndarray:
    """Raw frame under relative illumination eta: signal = level*eta with
    Poisson+read noise sigma^2 = read^2 + signal/gain (noise scales ~sqrt(eta),
    not eta)."""
    signal = level * eta
    return signal + rng.normal(0.0, 1.0, eta.shape) * np.sqrt(read**2 + signal / gain)


def gaussian_source(shape, x0, y0, amplitude, sigma) -> np.ndarray:
    y, x = np.indices(shape, dtype=float)
    return amplitude * np.exp(-((x - x0) ** 2 + (y - y0) ** 2) / (2.0 * sigma**2))


def edge_distance(shape) -> np.ndarray:
    h, w = shape
    y, x = np.indices(shape)
    return np.minimum.reduce([x, y, w - 1 - x, h - 1 - y]).astype(float)


def family(name: str, rng: np.random.Generator, shape, eta_i: float, eta_v: float):
    """Return (data, truth, description) for one registered family."""
    h, w = shape
    y, x = np.indices(shape, dtype=float)
    data = sky(rng, shape)
    truth = np.zeros(shape, dtype=np.uint8)
    if name == "null_flat":
        desc = "no artifact"
    elif name == "null_edge_source":
        data += gaussian_source(shape, 0.0, h / 2, 3000.0, 14.0)
        desc = "bright extended source centred on the left edge; no artifact"
    elif name == "null_illuminated_vignetting":
        e = edge_distance(shape)
        eta = 1.0 - 0.4 * np.clip(1.0 - e / 48.0, 0.0, 1.0) ** 2  # eta >= 0.6 >= eta_v
        data = illuminated(rng, eta)
        truth = labels_from_eta(eta, eta_i, eta_v)
        desc = "smooth raw vignetting with eta >= 0.6; usable science"
    elif name == "strip_sharp":
        width = int(rng.integers(5, max(6, w // 4)))
        data[:, :width] = rng.normal(0.0, 1.0, (h, width)) * 5.0
        truth[:, :width] = I
        desc = f"sharp unexposed left strip of width {width} (read noise only)"
    elif name == "corners_plateau":
        c = int(rng.integers(8, max(9, min(h, w) // 4)))
        for ys in (slice(0, c), slice(h - c, h)):
            for xs in (slice(0, c), slice(w - c, w)):
                data[ys, xs] = 50.0 + rng.normal(0.0, 1.0, (c, c))
                truth[ys, xs] = I
        desc = f"four {c}px corner plateaus"
    elif name == "circular_stop_raw":
        r0 = 0.46 * min(h, w)
        r = np.hypot(x - (w - 1) / 2, y - (h - 1) / 2)
        eta = np.clip((r0 + 6.0 - r) / 12.0, 0.0, 1.0)  # 12px ramp
        data = illuminated(rng, eta)
        truth = labels_from_eta(eta, eta_i, eta_v)
        desc = "circular field stop with a 12 px illumination ramp (raw)"
    elif name == "circular_stop_calibrated":
        r0 = 0.46 * min(h, w)
        r = np.hypot(x - (w - 1) / 2, y - (h - 1) / 2)
        eta = np.clip((r0 + 6.0 - r) / 12.0, 0.02, 1.0)
        data = illuminated(rng, eta) / eta  # flat division amplifies noise ~1/sqrt(eta)
        truth = labels_from_eta(eta, eta_i, eta_v)
        desc = "circular field stop after flat division (noise amplified outside)"
    elif name == "strip_amplified":
        width = int(rng.integers(16, max(17, w // 4)))
        data[:, :width] = 300.0 + rng.normal(0.0, 60.0, (h, width))
        truth[:, :width] = I
        desc = f"dark high-variance left strip of width {width}"
    elif name == "strip_with_nan":
        width = int(rng.integers(8, max(9, w // 5)))
        data[:, :width] = 0.0
        truth[:, :width] = I
        data[: max(2, h // 20), :] = np.nan
        truth[: max(2, h // 20), :] = U0
        desc = "zero strip plus undefined top rows"
    else:
        raise ValueError(name)
    truth[~np.isfinite(data)] = U0
    return data, truth, desc


FAMILIES = (
    "null_flat",
    "null_edge_source",
    "null_illuminated_vignetting",
    "strip_sharp",
    "corners_plateau",
    "circular_stop_raw",
    "circular_stop_calibrated",
    "strip_amplified",
    "strip_with_nan",
)


def write_corpus(
    out: Path,
    seeds: int,
    eta_i: float,
    eta_v: float,
    dtype: str,
    group_dirs: bool = False,
    seed_offset: int = 0,
    truth_dir: Path | None = None,
) -> dict:
    """Write fixtures; seeds run from seed_offset (held-out corpora use a
    disjoint range). Truth goes to truth_dir, default ``out/truth``."""
    truth_root = truth_dir if truth_dir is not None else out / "truth"
    out.mkdir(parents=True, exist_ok=True)
    truth_root.mkdir(parents=True, exist_ok=True)
    manifest = {
        "generator": GENERATOR_VERSION,
        "eta_i": eta_i,
        "eta_v": eta_v,
        "seed_offset": seed_offset,
        "files": [],
    }
    for fam in FAMILIES:
        for seed in range(seed_offset, seed_offset + seeds):
            rng = np.random.default_rng([zlib.crc32(fam.encode()), seed])
            shape = (int(rng.integers(160, 260)), int(rng.integers(160, 260)))
            data, truth, desc = family(fam, rng, shape, eta_i, eta_v)
            name = f"{fam}_{seed:04d}.fits"
            if group_dirs:
                # One directory per seed stands in for an independent group
                # (e.g. a night) for group-bootstrap evaluation.
                name = f"group_{seed:04d}/{name}"
                (out / f"group_{seed:04d}").mkdir(exist_ok=True)
                (truth_root / f"group_{seed:04d}").mkdir(exist_ok=True)
            if dtype == "int16":
                finite = np.isfinite(data)
                stored = np.clip(np.round(data), -32767, 32767)
                stored[~finite] = -32768
                hdu = fits.PrimaryHDU(stored.astype(np.int16))
                hdu.header["BLANK"] = -32768
            else:
                hdu = fits.PrimaryHDU(data.astype(np.float32))
            hdu.header["OBJECT"] = ("SYNTHETIC", "not detector evidence")
            hdu.writeto(out / name, overwrite=True)
            th = fits.PrimaryHDU(truth)
            th.header["TSCHEME"] = ("VIQU0", "0=V 1=I 2=Q 3=U0")
            th.header["ETA_I"] = eta_i
            th.header["ETA_V"] = eta_v
            th.writeto(truth_root / name, overwrite=True)
            manifest["files"].append({"file": name, "family": fam, "seed": seed, "description": desc})
    (truth_root / "manifest.json").write_text(json.dumps(manifest, indent=2))
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("output", type=Path)
    parser.add_argument("--seeds", type=int, default=3)
    parser.add_argument("--eta-i", type=float, default=0.20)
    parser.add_argument("--eta-v", type=float, default=0.50)
    parser.add_argument("--dtype", choices=("float32", "int16"), default="float32")
    parser.add_argument("--group-dirs", action="store_true", help="one subdirectory per seed")
    parser.add_argument("--seed-offset", type=int, default=0, help="first seed (held-out corpora: disjoint range)")
    parser.add_argument("--truth-dir", type=Path, help="truth planes outside the input tree (default: OUTPUT/truth)")
    args = parser.parse_args()
    if not 0.0 <= args.eta_i < args.eta_v <= 1.0:
        parser.error("require 0 <= eta_i < eta_v <= 1")
    if args.seed_offset < 0:
        parser.error("--seed-offset must be non-negative")
    if args.truth_dir is not None:
        out, truth = args.output.resolve(), args.truth_dir.resolve()
        if out == truth or out in truth.parents or truth in out.parents:
            parser.error("--truth-dir must not overlap the output tree")
    manifest = write_corpus(
        args.output, args.seeds, args.eta_i, args.eta_v, args.dtype, args.group_dirs,
        args.seed_offset, args.truth_dir,
    )
    print(f"wrote {len(manifest['files'])} fixtures to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
