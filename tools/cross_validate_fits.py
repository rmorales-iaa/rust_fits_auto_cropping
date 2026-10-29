#!/usr/bin/env python3
"""Cross-check Rust FITS products with Astropy and optional fitsverify."""

from __future__ import annotations

import argparse
from collections import defaultdict
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import warnings
import hashlib
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
CARD_POLICY_IGNORED = {
    "SIMPLE",
    "XTENSION",
    "BITPIX",
    "NAXIS",
    "EXTEND",
    "PCOUNT",
    "GCOUNT",
    "BSCALE",
    "BZERO",
    "BLANK",
    "CHECKSUM",
    "DATASUM",
    "DATAMIN",
    "DATAMAX",
    "CREATOR",
    "DATE",
    "HISTORY",
}
CARD_POLICY_ADDED = {
    "science": {
        "CHECKSUM",
        "DATASUM",
        "CREATOR",
        "DATE",
        "HISTORY",
        "BLANK",
        "CLIPSHAP",
        "CLIPX0",
        "CLIPY0",
        "COMMENT",
    },
    "mask": {"CHECKSUM", "DATASUM", "CREATOR", "DATE", "HISTORY", "COMMENT"},
    "reason": {
        "CHECKSUM",
        "DATASUM",
        "CREATOR",
        "DATE",
        "HISTORY",
        "COMMENT",
        "RSCHEME",
    },
}


def open_one(path: Path) -> fits.HDUList:
    hdus = fits.open(path, mode="readonly", memmap=False, do_not_scale_image_data=True)
    if len(hdus) != 1:
        hdus.close()
        raise AssertionError(f"{path}: expected one HDU, found {len(hdus)}")
    if hdus[0].data is None or hdus[0].data.ndim != 2:
        hdus.close()
        raise AssertionError(f"{path}: expected a 2D image")
    return hdus


def read_primary_header_cards(path: Path) -> list[tuple[int, bytes]]:
    cards: list[tuple[int, bytes]] = []
    with path.open("rb") as stream:
        offset = 0
        while True:
            block = stream.read(2880)
            if len(block) != 2880:
                raise AssertionError(f"{path}: truncated FITS header while validating")
            for index in range(36):
                start = index * 80
                card = block[start : start + 80]
                cards.append((offset + start, card))
                if card[:3] == b"END" and card[3:8] == b"     ":
                    return cards
            offset += 2880


def normalize_header_for_crosscheck(
    path: Path,
    temporary_directory: Path,
    malformed_offsets: set[int] = frozenset(),
    malformed_cards: set[bytes] = frozenset(),
) -> Path:
    """Normalize a disposable copy for Astropy; the science FITS stays untouched."""
    cards = read_primary_header_cards(path)
    repairs = [
        (offset, card)
        for offset, card in cards
        if offset in malformed_offsets
        or card.startswith(b"HIERARCH ")
        or card in malformed_cards
    ]
    if not repairs:
        return path

    identity = hashlib.sha256(str(path.resolve()).encode()).hexdigest()[:12]
    normalized = temporary_directory / f"{identity}-{path.name}"
    shutil.copyfile(path, normalized)
    with normalized.open("r+b") as stream:
        for offset, original in repairs:
            text = "".join(chr(byte) if 32 <= byte <= 126 else "?" for byte in original).rstrip()
            replacement = (f"COMMENT {text}".encode("ascii")[:80]).ljust(80, b" ")
            stream.seek(offset)
            stream.write(replacement)
    return normalized


def card_at_offset(path: Path, offset: int) -> bytes:
    with path.open("rb") as stream:
        stream.seek(offset)
        card = stream.read(80)
    if len(card) != 80:
        raise AssertionError(f"{path}: malformed-card offset is outside the header")
    return card


def verify_checksums(path: Path, hdus: fits.HDUList, required: bool = True) -> None:
    hdu = hdus[0]
    if "CHECKSUM" not in hdu.header or "DATASUM" not in hdu.header:
        if required:
            raise AssertionError(f"{path}: CHECKSUM/DATASUM card missing")
        return
    if hdu.verify_checksum() != 1 or hdu.verify_datasum() != 1:
        raise AssertionError(f"{path}: Astropy checksum verification failed")


def ones_complement_sum(data: bytes) -> int:
    total = 0
    for offset in range(0, len(data), 4):
        total += int.from_bytes(data[offset : offset + 4].ljust(4, b"\0"), "big")
        total = (total & 0xFFFFFFFF) + (total >> 32)
    while total >> 32:
        total = (total & 0xFFFFFFFF) + (total >> 32)
    return total


def verify_raw_checksums(path: Path, header: fits.Header) -> None:
    """Verify checksums against original product bytes, including preserved raw cards."""
    data = path.read_bytes()
    if len(data) % 2880:
        raise AssertionError(f"{path}: FITS file length is not a multiple of 2880 bytes")
    if ones_complement_sum(data) != 0xFFFFFFFF:
        raise AssertionError(f"{path}: raw FITS CHECKSUM does not sum to all ones")
    end_offset = next(
        (offset for offset, card in read_primary_header_cards(path) if card == b"END".ljust(80)),
        None,
    )
    if end_offset is None:
        raise AssertionError(f"{path}: FITS END card is missing")
    data_offset = ((end_offset + 80 + 2879) // 2880) * 2880
    if "DATASUM" not in header or ones_complement_sum(data[data_offset:]) != int(header["DATASUM"]):
        raise AssertionError(f"{path}: raw FITS DATASUM does not match its data unit")


def undefined_samples(data: np.ndarray, header: fits.Header) -> np.ndarray:
    """Undefined samples: NaN/inf for floats, BLANK for integer storage."""
    if np.issubdtype(data.dtype, np.floating):
        return ~np.isfinite(data)
    if "BLANK" in header:
        return data == header["BLANK"]
    return np.zeros(data.shape, dtype=bool)


def verify_masked_samples(
    expected: np.ndarray,
    expected_header: fits.Header,
    actual: np.ndarray,
    actual_header: fits.Header,
    newly_undefined: int,
    path: Path,
) -> None:
    """Check retained samples are identical and only report-counted samples became undefined.

    Without mask sidecars, the per-image report supplies the expected number of
    defined source samples that the product marks undefined. Every other sample
    must equal the source; originally undefined samples must stay undefined.
    """
    if expected.shape != actual.shape:
        raise AssertionError(f"{path}: product samples do not match the source region shape")
    source_undefined = undefined_samples(expected, expected_header)
    product_undefined = undefined_samples(actual, actual_header)
    if (source_undefined & ~product_undefined).any():
        raise AssertionError(f"{path}: an originally undefined sample became defined")
    marked = product_undefined & ~source_undefined
    if int(marked.sum()) != int(newly_undefined):
        raise AssertionError(
            f"{path}: {int(marked.sum())} samples newly undefined, report says {newly_undefined}"
        )
    retained = ~product_undefined
    if not np.array_equal(actual[retained], expected[retained]):
        raise AssertionError(f"{path}: retained samples differ from the source")


def verify_crop_samples(
    original: np.ndarray,
    actual: np.ndarray,
    x0: int,
    y0: int,
    path: Path,
    source_header: fits.Header | None = None,
    output_header: fits.Header | None = None,
    interior_masked: int = 0,
) -> None:
    """Check crop pixels against their input region without mask sidecars.

    `interior_masked` is the reported number of defined source samples set
    undefined inside the crop (crop_masked / bbox_mask actions); zero for a
    plain crop, which must equal its source region exactly.
    """
    expected = original[y0 : y0 + actual.shape[0], x0 : x0 + actual.shape[1]]
    if interior_masked:
        verify_masked_samples(
            expected, source_header or fits.Header(), actual, output_header or fits.Header(),
            interior_masked, path,
        )
        return
    equal_nan = np.issubdtype(np.result_type(expected.dtype, actual.dtype), np.floating)
    if expected.shape != actual.shape or not np.array_equal(
        actual, expected, equal_nan=equal_nan
    ):
        raise AssertionError(f"{path}: rectangular crop samples differ from the input region")


def verify_provenance(path: Path, header: fits.Header) -> None:
    if not str(header.get("CREATOR", "")).startswith("rust_fits_auto_cropping v"):
        raise AssertionError(f"{path}: CREATOR program/version card is missing")
    if not re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d", str(header.get("DATE", ""))):
        raise AssertionError(f"{path}: DATE product timestamp is missing or malformed")
    if not any("generated_utc=" in str(value) for value in header.get("HISTORY", [])):
        raise AssertionError(f"{path}: generated_utc HISTORY provenance is missing")


def verify_reason_codes(
    original: np.ndarray,
    source_header: fits.Header,
    exclusion_mask: np.ndarray,
    reason_codes: np.ndarray,
    is_crop: bool,
    path: Path,
) -> None:
    if original.shape != reason_codes.shape or exclusion_mask.shape != reason_codes.shape:
        raise AssertionError(f"{path}: reason-plane dimensions do not match source/mask")
    if reason_codes.dtype != np.dtype("uint8") or not np.isin(reason_codes, (0, 1, 2, 3, 4)).all():
        raise AssertionError(f"{path}: reason plane must be uint8 with codes 0..4")

    undefined = ~np.isfinite(original)
    if not np.issubdtype(original.dtype, np.floating) and "BLANK" in source_header:
        undefined |= original == source_header["BLANK"]
    if not np.array_equal(reason_codes == 3, undefined):
        raise AssertionError(f"{path}: code 3 does not match pre-existing undefined samples")

    defined_excluded = (exclusion_mask == 1) & ~undefined
    reason_excluded = (reason_codes == 1) | (reason_codes == 2)
    if not np.array_equal(reason_excluded, defined_excluded):
        raise AssertionError(f"{path}: reason exclusions disagree with the binary action mask")
    if not is_crop and not np.array_equal(reason_codes == 1, defined_excluded):
        raise AssertionError(f"{path}: mask-mode artifact reason codes are inconsistent")
    retained_defined = ~undefined & (exclusion_mask == 0)
    if not np.array_equal((reason_codes == 0) | (reason_codes == 4), retained_defined):
        raise AssertionError(f"{path}: retained-pixel reason codes are inconsistent")
    if not is_crop and (reason_codes == 4).any():
        raise AssertionError(f"{path}: mask-mode cannot retain detector-flagged pixels")


def verify_preserved_header(
    source: fits.Header,
    output: fits.Header,
    path: Path,
    product_kind: str,
) -> None:
    """Check all source cards outside the documented science-product policy."""
    if product_kind not in {"science", "mask", "reason"}:
        raise ValueError(f"unknown FITS product kind: {product_kind}")

    def preserved(header: fits.Header) -> dict[str, list[tuple[object, object]]]:
        cards: dict[str, list[tuple[object, object]]] = defaultdict(list)
        for card in header.cards:
            key = card.keyword
            if (
                not key
                or key in CARD_POLICY_IGNORED
                or re.fullmatch(r"NAXIS\d+|ZNAXIS\d+", key)
                or key.startswith("CLIP")
                or WCS_KEY.fullmatch(key)
                or (product_kind in {"mask", "reason"} and key == "BUNIT")
            ):
                continue
            cards[key].append((card.value, card.comment))
        return cards

    expected = preserved(source)
    actual = preserved(output)
    for key, source_cards in expected.items():
        if actual.get(key) != source_cards:
            raise AssertionError(f"{path}: source metadata card {key} changed or disappeared")
    source_keys = set(source)
    for key in output:
        if key in source_keys or key in CARD_POLICY_ADDED[product_kind]:
            continue
        if (
            re.fullmatch(r"SIMPLE|XTENSION|BITPIX|NAXIS\d*|EXTEND|PCOUNT|GCOUNT", key)
            or key.startswith("CLIP") and product_kind == "science"
        ):
            continue
        raise AssertionError(f"{path}: unexpected output header card {key}")


def verify_passthrough_symlink(source: Path, output: Path) -> None:
    if not output.is_symlink():
        raise AssertionError(f"{output}: malformed/ignored input must be a symlink")
    if output.resolve(strict=True) != source.resolve(strict=True):
        raise AssertionError(f"{output}: pass-through symlink does not target its input")


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
        elif isinstance(expected, (int, float, np.number)) and isinstance(
            output[key], (int, float, np.number)
        ):
            if not np.isclose(output[key], expected, rtol=1e-12, atol=1e-15):
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


def verify_tree(
    input_dir: Path, output_dir: Path, fitsverify: str | None
) -> tuple[int, int, int, int]:
    images = sorted((output_dir / "images").rglob("*"))
    images = [path for path in images if path.is_file()]
    summary = json.loads((output_dir / "summary.json").read_text())
    records = {
        os.path.normpath(record["output_image"]): record
        for record in summary["files"]
        if record.get("output_image")
    }
    if summary.get("schema_version", 0) >= 8 and summary["output_images"] != len(images):
        raise AssertionError(
            f"summary reports {summary['output_images']} output images but found {len(images)}"
        )
    if summary.get("schema_version", 0) >= 8 and not summary["dry_run"] and len(images) != summary["total"]:
        raise AssertionError(
            f"one-to-one image count failed: {summary['total']} inputs, {len(images)} outputs"
        )
    checked_images = checked_masks = checked_reason_masks = passthrough_count = 0
    source_repair_temporaries: list[tempfile.TemporaryDirectory[str]] = []
    for output_path in images:
        record = records.get(os.path.normpath(str(output_path.absolute())))
        if record is None:
            raise AssertionError(f"{output_path}: no matching per-input summary record")
        relative = Path(record["input"])
        source_path = input_dir / relative
        if not source_path.is_file():
            raise AssertionError(f"no matching input for output {output_path}")
        status = record["status"]
        if status == "ignored" or (status == "error" and output_path.is_symlink()):
            verify_passthrough_symlink(source_path, output_path)
            passthrough_count += 1
            checked_images += 1
            continue
        mask_path = output_dir / "masks" / relative
        reason = record.get("reason", "")
        temporary = tempfile.TemporaryDirectory(prefix="fits-header-crosscheck-")
        source_repair_temporaries.append(temporary)
        malformed_cards: set[bytes] = set()
        malformed_match = re.search(
            r"ignored malformed non-structural FITS header card at byte (\d+)", reason
        )
        if malformed_match:
            malformed_cards.add(card_at_offset(source_path, int(malformed_match.group(1))))
        validation_dir = Path(temporary.name)
        source_check_path = normalize_header_for_crosscheck(
            source_path, validation_dir, malformed_cards=malformed_cards
        )
        output_check_path = normalize_header_for_crosscheck(
            output_path, validation_dir, malformed_cards=malformed_cards
        )
        mask_check_path = normalize_header_for_crosscheck(
            mask_path, validation_dir, malformed_cards=malformed_cards
        ) if mask_path.exists() else mask_path
        reason_path = output_dir / "reason_masks" / relative
        reason_check_path = normalize_header_for_crosscheck(
            reason_path, validation_dir, malformed_cards=malformed_cards
        ) if reason_path.exists() else reason_path
        with open_one(source_check_path) as source, open_one(output_check_path) as output:
            if output_path.is_symlink():
                if output_path.resolve() != source_path.resolve():
                    raise AssertionError(f"{output_path}: kept-image symlink does not target its input")
            else:
                verify_provenance(output_path, output[0].header)
            verify_preserved_header(
                source[0].header,
                output[0].header,
                output_path,
                "science",
            )
            x0 = int(output[0].header.get("CLIPX0", 0))
            y0 = int(output[0].header.get("CLIPY0", 0))
            original = np.asarray(source[0].data)
            actual = np.asarray(output[0].data)
            crop_shape = output[0].header.get("CLIPSHAP")
            if crop_shape and crop_shape != "RECTANGLE":
                raise AssertionError(f"{output_path}: crop output is not a rectangle")
            if source[0].header["BITPIX"] != output[0].header["BITPIX"]:
                raise AssertionError(f"{output_path}: shape or BITPIX changed")
            is_crop = bool(crop_shape or x0 or y0 or original.shape != actual.shape)
            verify_wcs(source[0].header, output[0].header, output_path, x0, y0, is_crop)
            if is_crop:
                expected = original[y0 : y0 + actual.shape[0], x0 : x0 + actual.shape[1]]
                if expected.shape != actual.shape:
                    raise AssertionError(f"{output_path}: crop bounds exceed the source image")
                header = output[0].header
                if (
                    int(header.get("NAXIS", -1)) != 2
                    or int(header.get("NAXIS1", -1)) != actual.shape[1]
                    or int(header.get("NAXIS2", -1)) != actual.shape[0]
                ):
                    raise AssertionError(f"{output_path}: NAXIS cards disagree with crop dimensions")
                verify_crop_samples(
                    original, actual, x0, y0, output_path,
                    source[0].header, output[0].header,
                    int(record.get("crop_interior_masked_pixels", 0) or 0),
                )
            elif original.shape != actual.shape:
                raise AssertionError(f"{output_path}: dimensions changed without crop provenance")
            if mask_path.exists():
                verify_raw_checksums(output_path, output[0].header)
                with open_one(mask_check_path) as mask_hdus:
                    verify_raw_checksums(mask_path, mask_hdus[0].header)
                    verify_provenance(mask_path, mask_hdus[0].header)
                    verify_preserved_header(
                        source[0].header,
                        mask_hdus[0].header,
                        mask_path,
                        "mask",
                    )
                    if "BUNIT" in mask_hdus[0].header:
                        raise AssertionError(f"{mask_path}: binary mask must be dimensionless")
                    mask = np.asarray(mask_hdus[0].data)
                    if mask.shape != original.shape or not np.isin(mask, (0, 1)).all():
                        raise AssertionError(f"{mask_path}: invalid uint8 mask geometry or values")
                    selected = mask == 1
                    if not selected.any():
                        raise AssertionError(f"{mask_path}: empty mask product")
                    if is_crop:
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
                if reason_path.exists():
                    with open_one(reason_check_path) as reason_hdus:
                        verify_raw_checksums(reason_path, reason_hdus[0].header)
                        verify_provenance(reason_path, reason_hdus[0].header)
                        verify_preserved_header(
                            source[0].header,
                            reason_hdus[0].header,
                            reason_path,
                            "reason",
                        )
                        reason = reason_hdus[0]
                        if reason.header.get("RSCHEME") != "PIXACT2":
                            raise AssertionError(f"{reason_path}: unknown reason-code scheme")
                        if "BUNIT" in reason.header:
                            raise AssertionError(f"{reason_path}: reason plane must be dimensionless")
                        if reason.data is None or reason.data.shape != original.shape:
                            raise AssertionError(f"{reason_path}: reason-plane geometry differs from source")
                        verify_reason_codes(
                            original,
                            source[0].header,
                            mask,
                            np.asarray(reason.data),
                            is_crop,
                            reason_path,
                        )
                        verify_wcs(
                            source[0].header,
                            reason.header,
                            reason_path,
                            0,
                            0,
                            False,
                        )
                    checked_reason_masks += 1
            else:
                if (output_dir / "reason_masks" / relative).exists():
                    raise AssertionError(f"{output_path}: reason plane exists without a binary mask")
                if not output_path.is_symlink():
                    verify_raw_checksums(output_path, output[0].header)
                if is_crop:
                    pass  # samples verified above against the reported interior count
                elif output_path.is_symlink():
                    if not np.array_equal(actual, original, equal_nan=True):
                        raise AssertionError(f"{output_path}: kept image differs from its input")
                else:
                    # Dimension-preserving mask product: masked samples are
                    # undefined, all others identical (no sidecar needed).
                    verify_masked_samples(
                        original, source[0].header, actual, output[0].header,
                        int(record.get("masked_pixels", 0) or 0), output_path,
                    )
        if fitsverify:
            reason_path = output_dir / "reason_masks" / relative
            for path in (
                output_check_path,
                *((mask_check_path,) if mask_path.exists() else ()),
                *((reason_check_path,) if reason_path.exists() else ()),
            ):
                result = subprocess.run([fitsverify, str(path)], capture_output=True, text=True, check=False)
                transcript = result.stdout + result.stderr
                if "*** Error" in transcript or "Verification found" in transcript and re.search(r"\b[1-9]\d* error", transcript):
                    raise AssertionError(f"fitsverify rejected {path}:\n{transcript}")
        checked_images += 1
    return checked_images, checked_masks, checked_reason_masks, passthrough_count


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True, help="one output/runs/<run-id> directory")
    parser.add_argument("--fitsverify", default=shutil.which("fitsverify"))
    args = parser.parse_args()
    try:
        images, masks, reason_masks, passthrough = verify_tree(
            args.input_dir, args.output_dir, args.fitsverify
        )
    except Exception as error:  # surface the first concrete mismatch with a failure code
        print(f"cross-validation failed: {error}", file=sys.stderr)
        return 1
    if passthrough:
        print(
            f"Validated {images} output image entries ({passthrough} pass-through symlinks), "
            f"{masks} masks and {reason_masks} reason planes"
        )
    else:
        print(f"Astropy verified {images} science images, {masks} masks and {reason_masks} reason planes")
    if args.fitsverify:
        print(
            f"fitsverify checked all parsed science/mask/reason products using {args.fitsverify}; "
            "pass-through symlinks were validated by target"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
