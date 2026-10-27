#!/usr/bin/env python3
"""Cross-check Rust FITS products with Astropy and optional fitsverify."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path

import numpy as np
from astropy.io import fits
from astropy.wcs import WCS


WCS_KEY = re.compile(
    r"^(?:WCSAXES|WCSNAME|RADESYS|RADECSYS|EQUINOX|LONPOLE|LATPOLE|"
    r"CTYPE\d[A-Z]?|CRPIX\d[A-Z]?|CRVAL\d[A-Z]?|CDELT\d[A-Z]?|CUNIT\d[A-Z]?|"
    r"CROTA\d[A-Z]?|(?:CD|PC)\d_\d[A-Z]?|(?:A|B|AP|BP)(?:_ORDER|_\d+_\d+))$"
)


def open_one(path: Path) -> fits.HDUList:
    hdus = fits.open(path, mode="readonly", memmap=False, do_not_scale_image_data=True)
    if len(hdus) != 1:
        hdus.close()
        raise AssertionError(f"{path}: expected one HDU, found {len(hdus)}")
    if hdus[0].data is None or hdus[0].data.ndim != 2:
        hdus.close()
        raise AssertionError(f"{path}: expected a 2D image")
    return hdus


def verify_checksums(path: Path, hdus: fits.HDUList, required: bool = True) -> None:
    hdu = hdus[0]
    if "CHECKSUM" not in hdu.header or "DATASUM" not in hdu.header:
        if required:
            raise AssertionError(f"{path}: CHECKSUM/DATASUM card missing")
        return
    if hdu.verify_checksum() != 1 or hdu.verify_datasum() != 1:
        raise AssertionError(f"{path}: Astropy checksum verification failed")


def verify_provenance(path: Path, header: fits.Header) -> None:
    if not str(header.get("CREATOR", "")).startswith("fits-auto-clipping v"):
        raise AssertionError(f"{path}: CREATOR program/version card is missing")
    if not re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d", str(header.get("DATE", ""))):
        raise AssertionError(f"{path}: DATE product timestamp is missing or malformed")
    if not any("generated_utc=" in str(value) for value in header.get("HISTORY", [])):
        raise AssertionError(f"{path}: generated_utc HISTORY provenance is missing")


def verify_wcs(
    source: fits.Header, output: fits.Header, path: Path, x0: int, y0: int
) -> None:
    keys = {key for key in source if WCS_KEY.fullmatch(key)}
    for key in keys:
        expected = source[key]
        if key.startswith("CRPIX1"):
            expected -= x0
        elif key.startswith("CRPIX2"):
            expected -= y0
        if key not in output:
            raise AssertionError(f"{path}: WCS card {key} changed or disappeared")
        if key.startswith(("CRPIX1", "CRPIX2")):
            if not np.isclose(output[key], expected, rtol=0, atol=1e-12):
                raise AssertionError(f"{path}: WCS card {key} changed or disappeared")
        elif output[key] != expected:
            raise AssertionError(f"{path}: WCS card {key} changed or disappeared")
    if x0 or y0:
        old_wcs = WCS(source, relax=True)
        new_wcs = WCS(output, relax=True)
        height, width = output["NAXIS2"], output["NAXIS1"]
        points = np.array(
            [[0, 0], [width - 1, 0], [0, height - 1], [width - 1, height - 1],
             [(width - 1) / 2, (height - 1) / 2]],
            dtype=float,
        )
        old_world = old_wcs.all_pix2world(points + [x0, y0], 0)
        new_world = new_wcs.all_pix2world(points, 0)
        delta = new_world - old_world
        if delta.shape[1] >= 1:
            delta[:, 0] = (delta[:, 0] + 180.0) % 360.0 - 180.0
        if not np.allclose(delta, 0.0, rtol=0, atol=1e-8, equal_nan=True):
            raise AssertionError(f"{path}: WCS world coordinates changed across crop")


def verify_tree(input_dir: Path, output_dir: Path, fitsverify: str | None) -> tuple[int, int]:
    images = sorted((output_dir / "images").rglob("*"))
    images = [path for path in images if path.is_file()]
    checked_images = checked_masks = 0
    for output_path in images:
        relative = output_path.relative_to(output_dir / "images")
        source_path = input_dir / relative
        if not source_path.is_file():
            raise AssertionError(f"no matching input for output {output_path}")
        mask_path = output_dir / "masks" / relative
        with open_one(source_path) as source, open_one(output_path) as output:
            if output_path.is_symlink():
                if output_path.resolve() != source_path.resolve():
                    raise AssertionError(f"{output_path}: kept-image symlink does not target its input")
            else:
                verify_provenance(output_path, output[0].header)
            x0 = int(output[0].header.get("CLIPX0", 0))
            y0 = int(output[0].header.get("CLIPY0", 0))
            verify_wcs(source[0].header, output[0].header, output_path, x0, y0)
            original = np.asarray(source[0].data)
            actual = np.asarray(output[0].data)
            crop_shape = output[0].header.get("CLIPSHAP")
            if source[0].header["BITPIX"] != output[0].header["BITPIX"]:
                raise AssertionError(f"{output_path}: shape or BITPIX changed")
            is_crop = bool(crop_shape or x0 or y0 or original.shape != actual.shape)
            if is_crop:
                expected = original[y0 : y0 + actual.shape[0], x0 : x0 + actual.shape[1]]
                if expected.shape != actual.shape:
                    raise AssertionError(f"{output_path}: crop bounds exceed the source image")
                yy, xx = np.indices(actual.shape)
                if crop_shape == "CIRCLE":
                    cx = float(output[0].header["CLIPCX"])
                    cy = float(output[0].header["CLIPCY"])
                    radius = float(output[0].header["CLIPRAD"])
                    inside = (xx + x0 - cx) ** 2 + (yy + y0 - cy) ** 2 <= radius**2
                    expected = expected.copy()
                    if np.issubdtype(actual.dtype, np.floating):
                        expected[~inside] = np.nan
                        if not np.array_equal(expected, actual, equal_nan=True):
                            raise AssertionError(f"{output_path}: circular crop samples do not match")
                    else:
                        blank = output[0].header.get("BLANK")
                        if blank is None:
                            raise AssertionError(f"{output_path}: integer circular crop has no BLANK")
                        expected[~inside] = blank
                        if not np.array_equal(expected, actual):
                            raise AssertionError(f"{output_path}: circular crop samples do not match")
                elif crop_shape == "RECTANGLE":
                    if not np.array_equal(expected, actual):
                        raise AssertionError(f"{output_path}: rectangular crop samples changed")
                else:
                    raise AssertionError(f"{output_path}: missing/unsupported crop geometry {crop_shape}")
            elif original.shape != actual.shape:
                raise AssertionError(f"{output_path}: dimensions changed without crop provenance")
            if mask_path.exists():
                verify_checksums(output_path, output)
                with open_one(mask_path) as mask_hdus:
                    verify_checksums(mask_path, mask_hdus)
                    verify_provenance(mask_path, mask_hdus[0].header)
                    mask = np.asarray(mask_hdus[0].data)
                    if mask.shape != original.shape or not np.isin(mask, (0, 1)).all():
                        raise AssertionError(f"{mask_path}: invalid uint8 mask geometry or values")
                    selected = mask == 1
                    if not selected.any():
                        raise AssertionError(f"{mask_path}: empty mask product")
                    if is_crop:
                        yy_source, xx_source = np.indices(original.shape)
                        if crop_shape == "CIRCLE":
                            keep = (xx_source - float(output[0].header["CLIPCX"])) ** 2 + (
                                yy_source - float(output[0].header["CLIPCY"])
                            ) ** 2 <= float(output[0].header["CLIPRAD"]) ** 2
                        else:
                            keep = np.zeros(original.shape, dtype=bool)
                            keep[y0 : y0 + actual.shape[0], x0 : x0 + actual.shape[1]] = True
                        if not np.array_equal(selected, ~keep):
                            raise AssertionError(f"{mask_path}: exclusion mask disagrees with crop geometry")
                    elif np.issubdtype(actual.dtype, np.floating):
                        if not np.isnan(actual[selected]).all():
                            raise AssertionError(f"{output_path}: clipped floating pixels are not NaN")
                        if not np.array_equal(actual[~selected], original[~selected], equal_nan=True):
                            raise AssertionError(f"{output_path}: retained floating values changed")
                    else:
                        blank = output[0].header.get("BLANK")
                        if blank is None or not np.all(actual[selected] == blank):
                            raise AssertionError(f"{output_path}: integer BLANK values do not match mask")
                        if not np.array_equal(actual[~selected], original[~selected]):
                            raise AssertionError(f"{output_path}: retained integer values changed")
                    verify_wcs(source[0].header, mask_hdus[0].header, mask_path, 0, 0)
                checked_masks += 1
            else:
                verify_checksums(output_path, output, required=False)
                if not np.array_equal(actual, original, equal_nan=True):
                    raise AssertionError(f"{output_path}: kept image differs from its input")
        if fitsverify:
            for path in (output_path, *((mask_path,) if mask_path.exists() else ())):
                result = subprocess.run([fitsverify, str(path)], capture_output=True, text=True, check=False)
                transcript = result.stdout + result.stderr
                if "*** Error" in transcript or "Verification found" in transcript and re.search(r"\b[1-9]\d* error", transcript):
                    raise AssertionError(f"fitsverify rejected {path}:\n{transcript}")
        checked_images += 1
    return checked_images, checked_masks


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--fitsverify", default=shutil.which("fitsverify"))
    args = parser.parse_args()
    try:
        images, masks = verify_tree(args.input_dir, args.output_dir, args.fitsverify)
    except Exception as error:  # surface the first concrete mismatch with a failure code
        print(f"cross-validation failed: {error}", file=sys.stderr)
        return 1
    print(f"Astropy verified {images} science images and {masks} mask products")
    if args.fitsverify:
        print(f"fitsverify checked all science/mask outputs using {args.fitsverify}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
