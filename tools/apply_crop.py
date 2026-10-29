#!/usr/bin/env python3
"""Apply the crop recorded in a cropped science product to calibration frames.

rust_fits_auto_cropping writes crop provenance only on products that were
actually cropped:

    CROPPED  = T                  image was cropped; see CROPSEC
    CROPSEC  = '[x1:x2,y1:y2]'    kept section of the original (1-based, inclusive)
    ORIGNAX1 / ORIGNAX2           original image dimensions
    LTV1 / LTV2                   IRAF image = physical + LTV offsets
    ORIGSHA / ORIGFILE            identity of the original science frame

This tool reads those cards from ``--reference`` (a cropped product) and cuts
the same section from each calibration frame (bias, dark, flat) so the
calibrations stay pixel-aligned with the cropped science image. It checks that
each calibration frame has the original dimensions, copies the stored samples
exactly (no BSCALE/BZERO scaling is applied or changed), shifts CRPIX* and
LTV1/2, records the same provenance (with the calibration frame's own SHA-256)
plus CROPREF = the science frame's ORIGSHA, writes FITS checksums and verifies
the written file. Input files are never modified; existing outputs are only
replaced with --overwrite.
"""
from __future__ import annotations

import argparse
import hashlib
import re
import sys
from pathlib import Path

import numpy as np
from astropy.io import fits

SECTION = re.compile(r"\[(\d+):(\d+),(\d+):(\d+)\]")
CRPIX = re.compile(r"CRPIX([12])([A-Z]?)")
UNSUPPORTED_WCS = re.compile(r"CPDIS\d|CPERR\d|D2IMDIS\d|D2IMERR\d|DP[12]\..*|D2IM[12]\..*")


class CropError(ValueError):
    pass


def read_crop(reference: Path) -> tuple[tuple[int, int, int, int], tuple[int, int], str]:
    """(x0, y0, width, height) zero-based, original (NAXIS1, NAXIS2), science ORIGSHA."""
    header = fits.getheader(reference, 0)
    if header.get("CROPPED") is not True:
        raise CropError(f"{reference}: not a cropped product (CROPPED = T missing)")
    match = SECTION.fullmatch(str(header.get("CROPSEC", "")))
    if match is None:
        raise CropError(f"{reference}: CROPSEC missing or not of the form [x1:x2,y1:y2]")
    x1, x2, y1, y2 = map(int, match.groups())
    if not (1 <= x1 <= x2 and 1 <= y1 <= y2):
        raise CropError(f"{reference}: CROPSEC {match.group(0)} is not a valid section")
    try:
        original = (int(header["ORIGNAX1"]), int(header["ORIGNAX2"]))
    except KeyError as error:
        raise CropError(f"{reference}: missing {error.args[0]}") from None
    if x2 > original[0] or y2 > original[1]:
        raise CropError(f"{reference}: CROPSEC exceeds ORIGNAX1/ORIGNAX2")
    return (x1 - 1, y1 - 1, x2 - x1 + 1, y2 - y1 + 1), original, str(header.get("ORIGSHA", ""))


def crop_frame(
    source: Path,
    destination: Path,
    rect: tuple[int, int, int, int],
    original: tuple[int, int],
    science_sha: str,
) -> None:
    x0, y0, width, height = rect
    raw = source.read_bytes()
    with fits.open(source, do_not_scale_image_data=True, uint=False, memmap=False) as hdus:
        if len(hdus) != 1 or hdus[0].data is None or hdus[0].data.ndim != 2:
            raise CropError(f"{source}: expected a single-HDU 2D image")
        header = hdus[0].header.copy()
        if header.get("CROPPED") is True:
            raise CropError(f"{source}: already cropped; refusing to crop again")
        shape = (int(header["NAXIS1"]), int(header["NAXIS2"]))
        if shape != original:
            raise CropError(
                f"{source}: size {shape[0]}x{shape[1]} does not match the original "
                f"{original[0]}x{original[1]} of the cropped science frame"
            )
        for key in header:
            value = header[key]
            if UNSUPPORTED_WCS.fullmatch(key) or (
                key.startswith("CTYPE") and isinstance(value, str) and "-TAB" in value.upper()
            ):
                raise CropError(f"{source}: {key} cannot be shifted safely by a crop")
        data = np.array(hdus[0].data[y0 : y0 + height, x0 : x0 + width], copy=True)
        storage = {key: header[key] for key in ("BZERO", "BSCALE", "BLANK") if key in header}
    for key in list(header):
        match = CRPIX.fullmatch(key)
        if match:
            header[key] = float(header[key]) - (x0 if match.group(1) == "1" else y0)
    header["LTV1"] = (float(header.get("LTV1", 0.0)) - x0, "image = physical + LTV (IRAF)")
    header["LTV2"] = (float(header.get("LTV2", 0.0)) - y0, "image = physical + LTV (IRAF)")
    for key in ("DATAMIN", "DATAMAX", "CHECKSUM", "DATASUM"):
        header.remove(key, ignore_missing=True, remove_all=True)
    header["CROPPED"] = (True, "image was cropped; see CROPSEC")
    header["CROPSEC"] = (f"[{x0 + 1}:{x0 + width},{y0 + 1}:{y0 + height}]", "kept section of original image (1-based)")
    header["ORIGNAX1"] = (original[0], "NAXIS1 of the original image")
    header["ORIGNAX2"] = (original[1], "NAXIS2 of the original image")
    header["CROPUNDF"] = (0, "samples inside the crop set undefined")
    header["ORIGSHA"] = hashlib.sha256(raw).hexdigest()
    if len(source.name) <= 60 and source.name.isascii() and "'" not in source.name:
        header["ORIGFILE"] = source.name
    if science_sha:
        header["CROPREF"] = science_sha
    header.add_history("apply_crop.py: same crop as the science product with ORIGSHA = CROPREF")
    hdu = fits.PrimaryHDU(data=data, header=header, do_not_scale_image_data=True)
    # Astropy moves storage-scaling cards out of a new HDU's header; restore
    # them so the stored samples keep their original physical meaning.
    for key, value in storage.items():
        hdu.header[key] = value
    hdu.writeto(destination, checksum=True, overwrite=True)
    with fits.open(destination, do_not_scale_image_data=True, uint=False, checksum=True) as check:
        written = check[0].data
        if written.dtype != data.dtype or not np.array_equal(
            written.view(np.uint8), data.view(np.uint8)
        ):
            raise CropError(f"{destination}: written samples differ from the source section")
        for key in ("BZERO", "BSCALE", "BLANK"):
            if check[0].header.get(key) != storage.get(key):
                raise CropError(f"{destination}: storage card {key} changed")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--reference", type=Path, required=True, help="cropped science product")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument("calibrations", type=Path, nargs="+", help="calibration FITS frames")
    args = parser.parse_args(argv)
    try:
        rect, original, science_sha = read_crop(args.reference)
        args.output_dir.mkdir(parents=True, exist_ok=True)
        inputs = {path.resolve() for path in args.calibrations} | {args.reference.resolve()}
        for source in args.calibrations:
            destination = args.output_dir / f"{source.stem}_cropped{source.suffix}"
            if destination.resolve() in inputs:
                raise CropError(f"{destination}: would overwrite an input file")
            if destination.exists() and not args.overwrite:
                raise CropError(f"{destination}: exists; pass --overwrite")
            crop_frame(source, destination, rect, original, science_sha)
            print(f"{source} -> {destination} section [{rect[0] + 1}:{rect[0] + rect[2]},{rect[1] + 1}:{rect[1] + rect[3]}]")
    except (CropError, OSError, KeyError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
