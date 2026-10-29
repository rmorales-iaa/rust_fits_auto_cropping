#!/usr/bin/env python3
"""Out-of-model probes: artifacts absent from make_synthetic_corpus.py (plan R1.6).

Null families (no border invalid area, so any accepted removal is a false
alarm): cosmic rays (including hits on the outer pixels), satellite trails,
saturated edge stars with bleed columns, interior hot columns and crowded
edge-star fields.

Positive-artifact families (risk K2; written only with --truth-dir): additive
corner glow and edge light leak. Truth uses the corpus codes 0=V 1=I 2=Q.
An additive excess e is invalid (I) where e >= 3 sigma, uncertain (Q) where
0.5 sigma <= e < 3 sigma and valid (V) below. These thresholds are registered
here for evaluation only; they are not detector parameters.

Seeds run from --seed-offset (default 9000), disjoint from the development
(0..) and held-out (5000.., 7000..) corpora. Output is byte-reproducible.
"""
from __future__ import annotations

import argparse
import zlib
from pathlib import Path

import numpy as np
from astropy.io import fits

SKY, GAIN, READ = 1000.0, 1.5, 5.0
SIGMA = float(np.sqrt(READ**2 + SKY / GAIN))
I_SIGMA, Q_SIGMA = 3.0, 0.5
V, I, Q = 0, 1, 2


def sky(rng, h, w):
    return SKY + rng.normal(0, SIGMA, (h, w)), SIGMA


def star(img, y, x, flux, fwhm):
    s = fwhm / 2.3548
    yy, xx = np.mgrid[0:img.shape[0], 0:img.shape[1]]
    img += flux / (2 * np.pi * s * s) * np.exp(-((yy - y) ** 2 + (xx - x) ** 2) / (2 * s * s))


def cosmic(rng, h, w):
    img, s = sky(rng, h, w)
    n = int(rng.integers(200, 600))
    ys, xs = rng.integers(0, h, n), rng.integers(0, w, n)
    img[ys, xs] += rng.uniform(10, 200, n) * s
    # elongated hits, many on the outer 4 px
    for _ in range(40):
        y, x = int(rng.integers(0, h)), int(rng.choice([0, 1, 2, w - 3, w - 2, w - 1]))
        img[y : y + int(rng.integers(2, 6)), x] += rng.uniform(20, 200) * s
    return img


def trail(rng, h, w):
    img, s = sky(rng, h, w)
    yy, xx = np.mgrid[0:h, 0:w]
    a = rng.uniform(0, np.pi)
    c = rng.uniform(0.3, 0.7) * h
    d = np.abs((yy - c) * np.cos(a) - (xx - w / 2) * np.sin(a))
    img += rng.uniform(5, 50) * s * np.exp(-(d**2) / (2 * 1.5**2))
    return img


def edge_star_bleed(rng, h, w):
    img, s = sky(rng, h, w)
    x = float(rng.uniform(0.2, 0.8) * w)
    y = float(rng.uniform(-3, 6))
    star(img, y, x, 5e6, 3.5)
    img = np.minimum(img, 60000.0)
    xi = int(round(x))
    img[0 : int(rng.integers(20, 60)), xi - 1 : xi + 2] = 60000.0
    return img


def hot_column(rng, h, w):
    img, s = sky(rng, h, w)
    x = int(rng.integers(w // 4, 3 * w // 4))
    img[:, x] += rng.uniform(5, 30) * s
    return img


def crowded_edge(rng, h, w):
    img, s = sky(rng, h, w)
    for _ in range(int(rng.integers(60, 120))):
        side = rng.integers(0, 4)
        t = rng.uniform(0, 1)
        y, x = [
            (rng.uniform(0, 12), t * w),
            (h - rng.uniform(0, 12), t * w),
            (t * h, rng.uniform(0, 12)),
            (t * h, w - rng.uniform(0, 12)),
        ][side]
        star(img, y, x, rng.uniform(2e3, 2e5), rng.uniform(2.5, 4.5))
    return img


def _labels(excess):
    truth = np.full(excess.shape, V, dtype=np.uint8)
    truth[excess >= Q_SIGMA * SIGMA] = Q
    truth[excess >= I_SIGMA * SIGMA] = I
    return truth


def _interior_stars(rng, img):
    h, w = img.shape
    for _ in range(5):
        star(img, rng.uniform(0.3, 0.7) * h, rng.uniform(0.3, 0.7) * w, rng.uniform(5e3, 5e4), 3.0)


def corner_glow(rng, h, w):
    """Amplifier-like glow: exponential excess from one corner."""
    img, s = sky(rng, h, w)
    cy, cx = [(0, 0), (0, w - 1), (h - 1, 0), (h - 1, w - 1)][int(rng.integers(0, 4))]
    amp, scale = rng.uniform(20, 200) * s, rng.uniform(15, 50)
    yy, xx = np.mgrid[0:h, 0:w]
    excess = amp * np.exp(-np.hypot(yy - cy, xx - cx) / scale)
    img += excess
    _interior_stars(rng, img)
    return img, _labels(excess)


def light_leak(rng, h, w):
    """Edge light leak: excess decaying from one edge, modulated along it."""
    img, s = sky(rng, h, w)
    side = int(rng.integers(0, 4))
    amp, scale = rng.uniform(10, 100) * s, rng.uniform(10, 40)
    phase, period = rng.uniform(0, 2 * np.pi), rng.uniform(80, 300)
    yy, xx = np.mgrid[0:h, 0:w]
    dist, along = [(yy, xx), (h - 1 - yy, xx), (xx, yy), (w - 1 - xx, yy)][side]
    excess = amp * (1 + 0.3 * np.sin(2 * np.pi * along / period + phase)) * np.exp(-dist / scale)
    img += excess
    _interior_stars(rng, img)
    return img, _labels(excess)


NULL_FAMILIES = dict(
    cosmic=cosmic, trail=trail, edge_star_bleed=edge_star_bleed, hot_column=hot_column, crowded_edge=crowded_edge
)
POSITIVE_FAMILIES = dict(corner_glow=corner_glow, light_leak=light_leak)


def write(out: Path, frames: int, families: list[str], seed_offset: int, truth_dir: Path | None) -> int:
    missing = [name for name in families if name in POSITIVE_FAMILIES and truth_dir is None]
    if missing:
        raise ValueError(f"family {missing[0]} needs --truth-dir")
    out.mkdir(parents=True, exist_ok=True)
    count = 0
    for name in families:
        positive = name in POSITIVE_FAMILIES
        for seed in range(seed_offset, seed_offset + frames):
            rng = np.random.default_rng([zlib.crc32(name.encode()), seed])
            h, w = int(rng.integers(200, 300)), int(rng.integers(200, 300))
            file = f"{name}_{seed:05d}.fits"
            if positive:
                data, truth = POSITIVE_FAMILIES[name](rng, h, w)
                truth_dir.mkdir(parents=True, exist_ok=True)
                th = fits.PrimaryHDU(truth)
                th.header["TSCHEME"] = ("VIQU0", "0=V 1=I 2=Q 3=U0")
                th.header["ISIGMA"] = (I_SIGMA, "I where additive excess >= ISIGMA sigma")
                th.header["QSIGMA"] = (Q_SIGMA, "Q where QSIGMA sigma <= excess < ISIGMA")
                th.writeto(truth_dir / file, overwrite=True)
            else:
                data = NULL_FAMILIES[name](rng, h, w)
            fits.PrimaryHDU(data.astype(np.float32)).writeto(out / file, overwrite=True)
            count += 1
    return count


def main() -> int:
    every = list(NULL_FAMILIES) + list(POSITIVE_FAMILIES)
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("output", type=Path)
    parser.add_argument("frames", type=int, nargs="?", default=20, help="frames per family")
    parser.add_argument("--family", action="append", choices=every, help="repeatable; default: all null families")
    parser.add_argument("--seed-offset", type=int, default=9000)
    parser.add_argument("--truth-dir", type=Path, help="truth planes for positive families, outside OUTPUT")
    args = parser.parse_args()
    if args.frames < 1 or args.seed_offset < 0:
        parser.error("frames must be >= 1 and --seed-offset >= 0")
    if args.truth_dir is not None:
        out, truth = args.output.resolve(), args.truth_dir.resolve()
        if out == truth or out in truth.parents or truth in out.parents:
            parser.error("--truth-dir must not overlap the output tree")
    families = args.family or list(NULL_FAMILIES)
    try:
        n = write(args.output, args.frames, families, args.seed_offset, args.truth_dir)
    except ValueError as error:
        parser.error(str(error))
    print(f"wrote {n} frames to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
