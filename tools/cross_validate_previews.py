#!/usr/bin/env python3
"""Independently validate crop PNG scaling, colored outlines, geometry, and orientation."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
from astropy.io import fits
from astropy.visualization import ZScaleInterval
from PIL import Image

LABEL_HEIGHT = 18


def image_data(path: Path) -> tuple[np.ndarray, int | None]:
    with fits.open(path, memmap=False) as hdus:
        data = np.asarray(hdus[0].data, dtype=np.float64)
        blank = hdus[0].header.get("BLANK")
    if blank is not None:
        data[data == blank] = np.nan
    return data, blank


def expected_u8(value: float, lower: float, upper: float) -> int:
    span = upper - lower
    if not np.isfinite(value) or not np.isfinite(span) or span <= 0:
        return 0
    return int(np.rint(np.clip((value - lower) / span, 0, 1) * 255))


def crop_outline_masks(retained: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Return source-pixel masks for the green kept and red cropped outlines."""
    height, width = retained.shape
    padded = np.pad(retained, 1, mode="edge")
    retained_neighbor = np.zeros_like(retained)
    cropped_neighbor = np.zeros_like(retained)
    for dy in range(-1, 2):
        for dx in range(-1, 2):
            if dx == 0 and dy == 0:
                continue
            neighbor = padded[1 + dy : 1 + dy + height, 1 + dx : 1 + dx + width]
            retained_neighbor |= neighbor
            cropped_neighbor |= ~neighbor
    return retained & cropped_neighbor, ~retained & retained_neighbor


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("summary", type=Path, help="run summary.json")
    parser.add_argument("input_dir", type=Path, help="original input root")
    parser.add_argument("output_dir", type=Path, help="one output/runs/<run-id> directory")
    parser.add_argument("--samples", type=int, default=200, help="sample checks per PNG")
    args = parser.parse_args()
    summary = json.loads(args.summary.read_text())
    count = 0
    for record in summary["files"]:
        png_name = record.get("output_png")
        if not png_name:
            continue
        source_path = args.input_dir / record["input"]
        cropped_path = args.output_dir / "images" / record["input"]
        png_path = Path(png_name)
        source, _ = image_data(source_path)
        cropped, _ = image_data(cropped_path)
        header = fits.getheader(cropped_path)
        if header.get("CLIPSHAP") != "RECTANGLE":
            raise AssertionError(f"{cropped_path}: crop output is not a rectangular FITS image")
        x0 = int(header.get("CLIPX0", 0))
        y0 = int(header.get("CLIPY0", 0))
        if (
            int(header.get("NAXIS", -1)) != 2
            or int(header.get("NAXIS1", -1)) != cropped.shape[1]
            or int(header.get("NAXIS2", -1)) != cropped.shape[0]
        ):
            raise AssertionError(f"{cropped_path}: NAXIS cards disagree with the crop array shape")
        retained = np.zeros(source.shape, dtype=bool)
        retained[
            y0 : y0 + cropped.shape[0],
            x0 : x0 + cropped.shape[1],
        ] = True
        kept_outline, cropped_outline = crop_outline_masks(retained)
        with Image.open(png_path) as opened:
            png = np.asarray(opened.convert("RGB"))
        height, width = source.shape
        crop_height, crop_width = cropped.shape
        expected_shape = (max(height, crop_height) + LABEL_HEIGHT, width + crop_width)
        if png.shape != (*expected_shape, 3):
            raise AssertionError(f"{png_path}: dimensions {png.shape} != {expected_shape}")
        finite = source[np.isfinite(source)]
        lower, upper = ZScaleInterval().get_limits(finite)
        rng = np.random.default_rng(0)
        points = [(int(rng.integers(width)), int(rng.integers(height))) for _ in range(args.samples)]
        for x, y in points:
            actual = tuple(png[LABEL_HEIGHT + height - 1 - y, x])
            expected = expected_u8(float(source[y, x]), float(lower), float(upper))
            if kept_outline[y, x]:
                expected_rgb = (0, 255, 0)
            elif cropped_outline[y, x]:
                expected_rgb = (255, 0, 0)
            else:
                expected_rgb = (expected, expected, expected)
            if actual != expected_rgb:
                raise AssertionError(
                    f"{png_path}: source pixel ({x},{y}) is {actual}, expected {expected_rgb}"
                )
        # Validate pixels represented in both panels, including the FITS lower-origin transform.
        for _ in range(args.samples):
            x = int(rng.integers(crop_width))
            y = int(rng.integers(crop_height))
            global_x, global_y = x0 + x, y0 + y
            if not (0 <= global_x < width and 0 <= global_y < height):
                continue
            right_y = LABEL_HEIGHT + crop_height - 1 - y
            right_x = width + x
            if header.get("CLIPSHAP") == "CIRCLE":
                radius = float(header["CLIPRAD"])
                dx = global_x - float(header["CLIPCX"])
                dy = global_y - float(header["CLIPCY"])
                if dx * dx + dy * dy > radius * radius:
                    if tuple(png[right_y, right_x]) != (0, 0, 0):
                        raise AssertionError(f"{png_path}: circular exterior pixel is not black")
                    continue
            source_value = source[global_y, global_x]
            crop_value = cropped[y, x]
            if np.isfinite(crop_value):
                right = tuple(png[right_y, right_x])
                expected = expected_u8(float(source_value), float(lower), float(upper))
                if right != (expected, expected, expected):
                    raise AssertionError(
                        f"{png_path}: crop panel mismatch at source ({global_x},{global_y})"
                    )
        count += 1
    print(f"Astropy ZScale and PNG geometry verified for {count} preview(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
