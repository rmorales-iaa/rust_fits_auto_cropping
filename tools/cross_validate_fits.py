#!/usr/bin/env python3
"""Cross-check Rust FITS products with Astropy and optional fitsverify."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import warnings
from pathlib import Path

import numpy as np
from astropy.io import fits
from astropy.wcs import WCS


WCS_KEY = re.compile(
    r"^(?:WCSAXES[A-Z]?|WCSNAME[A-Z]?|RADESYS[A-Z]?|RADECSYS[A-Z]?|EQUINOX[A-Z]?|"
    r"LONPOLE[A-Z]?|LATPOLE[A-Z]?|CTYPE\d{1,2}[A-Z]?|CRPIX\d{1,2}[A-Z]?|"
    r"CRVAL\d{1,2}[A-Z]?|CDELT\d{1,2}[A-Z]?|CUNIT\d{1,2}[A-Z]?|"
    r"CROTA\d{1,2}[A-Z]?|(?:CD|PC)\d{1,2}_\d{1,2}[A-Z]?|"
    r"(?:PV|PS)\d{1,2}_\d{1,2}[A-Z]?|(?:A|B|AP|BP)(?:_ORDER|_\d+_\d+))$"
)
WCS_ALT_SUFFIX = re.compile(
    r"^(?:WCSAXES|WCSNAME|RADESYS|RADECSYS|EQUINOX|LONPOLE|LATPOLE)[A-Z]$|"
    r"^(?:CTYPE|CRPIX|CRVAL|CDELT|CUNIT|CROTA)\d{1,2}[A-Z]$|"
    r"^(?:CD|PC|PV|PS)\d{1,2}_\d{1,2}[A-Z]$"
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
    if not str(header.get("CREATOR", "")).startswith("rust_fits_auto_cropping v"):
        raise AssertionError(f"{path}: CREATOR program/version card is missing")
    if not re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d", str(header.get("DATE", ""))):
        raise AssertionError(f"{path}: DATE product timestamp is missing or malformed")
    if not any("generated_utc=" in str(value) for value in header.get("HISTORY", [])):
        raise AssertionError(f"{path}: generated_utc HISTORY provenance is missing")


def verify_wcs(
    source: fits.Header,
    output: fits.Header,
    path: Path,
    x0: int,
    y0: int,
    check_coordinates: bool,
) -> None:
    keys = {key for key in source if WCS_KEY.fullmatch(key)}
    for key in keys:
        expected = source[key]
        if re.fullmatch(r"CRPIX1[A-Z]?", key):
            expected -= x0
        elif re.fullmatch(r"CRPIX2[A-Z]?", key):
            expected -= y0
        if key not in output:
            raise AssertionError(f"{path}: WCS card {key} changed or disappeared")
        if re.fullmatch(r"CRPIX[12][A-Z]?", key):
            if not np.isclose(output[key], expected, rtol=0, atol=1e-12):
                raise AssertionError(f"{path}: WCS card {key} changed or disappeared")
        elif output[key] != expected:
            raise AssertionError(f"{path}: WCS card {key} changed or disappeared")
    if not check_coordinates or not keys:
        return

    alternate_codes = sorted(
        {
            match.group()[-1]
            for key in keys
            if (match := WCS_ALT_SUFFIX.fullmatch(key)) is not None
        }
    )
    for code in [" ", *alternate_codes]:
        old_wcs = WCS(source, key=code, relax=True)
        new_wcs = WCS(output, key=code, relax=True)
        height, width = int(output["NAXIS2"]), int(output["NAXIS1"])
        x_values = np.linspace(0, width - 1, min(width, 9))
        y_values = np.linspace(0, height - 1, min(height, 9))
        points = np.array([(x, y) for y in y_values for x in x_values], dtype=float)
        source_points = points + np.array([x0, y0], dtype=float)
        old_world = old_wcs.all_pix2world(source_points, 0)
        new_world = new_wcs.all_pix2world(points, 0)
        valid = np.isfinite(old_world).all(axis=1)
        if not valid.any():
            raise AssertionError(f"{path}: WCS {code!r} has no finite sampled input coordinates")
        if not np.isfinite(new_world[valid]).all():
            raise AssertionError(f"{path}: WCS {code!r} made valid input coordinates non-finite")

        longitude_axis = int(old_wcs.wcs.lng)
        latitude_axis = int(old_wcs.wcs.lat)
        delta = new_world[valid] - old_world[valid]
        if longitude_axis >= 0:
            delta[:, longitude_axis] = (delta[:, longitude_axis] + 180.0) % 360.0 - 180.0
        if longitude_axis >= 0 and latitude_axis >= 0:
            old_lon = np.deg2rad(old_world[valid, longitude_axis])
            new_lon = np.deg2rad(new_world[valid, longitude_axis])
            old_lat = np.deg2rad(old_world[valid, latitude_axis])
            new_lat = np.deg2rad(new_world[valid, latitude_axis])
            dlon = (new_lon - old_lon + np.pi) % (2 * np.pi) - np.pi
            haversine = np.sin((new_lat - old_lat) / 2) ** 2 + np.cos(old_lat) * np.cos(
                new_lat
            ) * np.sin(dlon / 2) ** 2
            angular_error_deg = np.rad2deg(2 * np.arcsin(np.sqrt(np.clip(haversine, 0, 1))))
            if not np.all(angular_error_deg <= 1e-8):
                raise AssertionError(f"{path}: WCS {code!r} angular coordinates changed across crop")
            other_axes = [
                axis
                for axis in range(delta.shape[1])
                if axis not in (longitude_axis, latitude_axis)
            ]
            if other_axes and not np.all(np.abs(delta[:, other_axes]) <= 1e-8):
                raise AssertionError(f"{path}: WCS {code!r} non-angular coordinates changed across crop")
        elif not np.all(np.abs(delta) <= 1e-8):
            raise AssertionError(f"{path}: WCS {code!r} coordinates changed across crop")

        try:
            with warnings.catch_warnings():
                warnings.filterwarnings("ignore", message="All-NaN slice encountered", category=RuntimeWarning)
                old_roundtrip = old_wcs.all_world2pix(
                    old_world[valid], 0, tolerance=1e-6, maxiter=100, quiet=True
                )
                new_roundtrip = new_wcs.all_world2pix(
                    new_world[valid], 0, tolerance=1e-6, maxiter=100, quiet=True
                )
        except Exception as error:
            raise AssertionError(f"{path}: WCS {code!r} inverse transform failed: {error}") from error
        old_inverse_valid = np.isfinite(old_roundtrip).all(axis=1)
        new_inverse_valid = np.isfinite(new_roundtrip).all(axis=1)
        if not np.array_equal(old_inverse_valid, new_inverse_valid):
            raise AssertionError(f"{path}: WCS {code!r} inverse validity changed across crop")
        if not old_inverse_valid.any():
            warnings.warn(
                f"{path}: WCS {code!r} inverse residual unavailable; forward coordinates matched",
                RuntimeWarning,
                stacklevel=2,
            )
        elif not np.all(
            np.max(np.abs(old_roundtrip[old_inverse_valid] - source_points[valid][old_inverse_valid]), axis=1)
            <= 1e-6
        ):
            raise AssertionError(f"{path}: WCS {code!r} input inverse residual exceeds 1e-6 pixel")
        elif not np.all(
            np.max(np.abs(new_roundtrip[new_inverse_valid] - points[valid][new_inverse_valid]), axis=1)
            <= 1e-6
        ):
            raise AssertionError(f"{path}: WCS {code!r} output inverse residual exceeds 1e-6 pixel")


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
            original = np.asarray(source[0].data)
            actual = np.asarray(output[0].data)
            crop_shape = output[0].header.get("CLIPSHAP")
            if source[0].header["BITPIX"] != output[0].header["BITPIX"]:
                raise AssertionError(f"{output_path}: shape or BITPIX changed")
            is_crop = bool(crop_shape or x0 or y0 or original.shape != actual.shape)
            verify_wcs(source[0].header, output[0].header, output_path, x0, y0, is_crop)
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
                    verify_wcs(source[0].header, mask_hdus[0].header, mask_path, 0, 0, False)
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
    parser.add_argument("--output-dir", type=Path, required=True, help="one output/runs/<run-id> directory")
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
