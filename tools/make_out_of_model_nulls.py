#!/usr/bin/env python3
"""Out-of-model null probes: artifacts absent from make_synthetic_corpus.py.

Families: cosmic rays (including hits on the outer pixels), satellite trails,
saturated edge stars with bleed columns, interior hot columns and crowded
edge-star fields. None is a border invalid area, so any accepted removal is a
false alarm (plan Revision 3, R1.6). Seeds start at 9000 and are disjoint from
the development (0..) and held-out corpora.

Usage: make_out_of_model_nulls.py OUTPUT [FRAMES_PER_FAMILY]
"""
import sys, zlib
from pathlib import Path
import numpy as np
from astropy.io import fits

if len(sys.argv) < 2:
    sys.exit(__doc__)
out = Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
N = int(sys.argv[2]) if len(sys.argv) > 2 else 20
SKY, GAIN, READ = 1000.0, 1.5, 5.0

def sky(rng, h, w):
    sigma = np.sqrt(READ**2 + SKY / GAIN)
    return SKY + rng.normal(0, sigma, (h, w)), sigma

def star(img, y, x, flux, fwhm):
    s = fwhm / 2.3548
    yy, xx = np.mgrid[0:img.shape[0], 0:img.shape[1]]
    img += flux / (2*np.pi*s*s) * np.exp(-((yy-y)**2 + (xx-x)**2) / (2*s*s))

def cosmic(rng, h, w):
    img, s = sky(rng, h, w)
    n = int(rng.integers(200, 600))
    ys, xs = rng.integers(0, h, n), rng.integers(0, w, n)
    img[ys, xs] += rng.uniform(10, 200, n) * s
    # elongated hits, many on the outer 4 px
    for _ in range(40):
        y, x = int(rng.integers(0, h)), int(rng.choice([0, 1, 2, w-3, w-2, w-1]))
        img[y:y+int(rng.integers(2, 6)), x] += rng.uniform(20, 200) * s
    return img

def trail(rng, h, w):
    img, s = sky(rng, h, w)
    yy, xx = np.mgrid[0:h, 0:w]
    a = rng.uniform(0, np.pi); c = rng.uniform(0.3, 0.7) * h
    d = np.abs((yy - c) * np.cos(a) - (xx - w/2) * np.sin(a))
    img += rng.uniform(5, 50) * s * np.exp(-d**2 / (2 * 1.5**2))
    return img

def edge_star_bleed(rng, h, w):
    img, s = sky(rng, h, w)
    x = float(rng.uniform(0.2, 0.8) * w); y = float(rng.uniform(-3, 6))
    star(img, y, x, 5e6, 3.5)
    img = np.minimum(img, 60000.0)
    xi = int(round(x)); img[0:int(rng.integers(20, 60)), xi-1:xi+2] = 60000.0
    return img

def hot_column(rng, h, w):
    img, s = sky(rng, h, w)
    x = int(rng.integers(w//4, 3*w//4)); img[:, x] += rng.uniform(5, 30) * s
    return img

def crowded_edge(rng, h, w):
    img, s = sky(rng, h, w)
    for _ in range(int(rng.integers(60, 120))):
        side = rng.integers(0, 4); t = rng.uniform(0, 1)
        y, x = [(rng.uniform(0, 12), t*w), (h-rng.uniform(0, 12), t*w), (t*h, rng.uniform(0, 12)), (t*h, w-rng.uniform(0, 12))][side]
        star(img, y, x, rng.uniform(2e3, 2e5), rng.uniform(2.5, 4.5))
    return img

FAMS = dict(cosmic=cosmic, trail=trail, edge_star_bleed=edge_star_bleed, hot_column=hot_column, crowded_edge=crowded_edge)
for name, f in FAMS.items():
    for seed in range(N):
        rng = np.random.default_rng([zlib.crc32(name.encode()), 9000 + seed])
        h, w = int(rng.integers(200, 300)), int(rng.integers(200, 300))
        fits.PrimaryHDU(f(rng, h, w).astype(np.float32)).writeto(out / f"{name}_{seed:03d}.fits", overwrite=True)
print("ok")
