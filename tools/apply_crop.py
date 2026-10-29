#!/usr/bin/env python3
"""Apply the crop recorded in a cropped science product to calibration frames.

rust_fits_auto_cropping marks cropped products with MO_ cards:

    MO_CROP  = T                  cropped from the original frame
    MO_SEC   = '[x1:x2,y1:y2]'    kept section of the original (1-based, inclusive)
    MO_ONAX1 / MO_ONAX2           original image dimensions
    MO_X0 / MO_Y0                 zero-based origin of the section
    MO_SHA / MO_FILE              identity of the original science frame

This tool reads them from ``--reference`` and cuts the same section from each
calibration frame (bias, dark, flat), so the calibrations stay pixel-aligned
with the cropped science image. It follows the same header rule as the
science products: every original header card is copied byte for byte, except
NAXIS1/NAXIS2, CRPIX1/2[a] and existing LTV1/LTV2 (shifted by the crop origin)
and CHECKSUM/DATASUM; an MO_ group is appended (MO_REF = the science frame's
MO_SHA). The stored sample bytes of the section are copied unchanged (no
scaling is applied). Each frame must have the original dimensions and must not
already be cropped. Outputs are verified after writing; inputs are never
modified, and existing outputs are replaced only with --overwrite.
"""
from __future__ import annotations

import argparse
import hashlib
import re
import sys
import warnings
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
from astropy.io import fits

SECTION = re.compile(r"\[(\d+):(\d+),(\d+):(\d+)\]")
SHIFTED = re.compile(r"CRPIX([12])[A-Z]?|LTV([12])")
UNSUPPORTED_WCS = re.compile(r"CPDIS\d|CPERR\d|D2IMDIS\d|D2IMERR\d|DP[12]\..*|D2IM[12]\..*")
PROGRAM = "rust_fits_auto_cropping tools/apply_crop.py"


class CropError(ValueError):
    pass


# ---------------------------------------------------------------- FITS cards


def card_key(card: bytes) -> str:
    return card[:8].decode("ascii", "replace").rstrip()


def format_float(value: float) -> str:
    text = repr(float(value))
    if "e" in text:
        mantissa, exponent = text.split("e")
        if "." not in mantissa:
            mantissa += ".0"
        return f"{mantissa}E{exponent}"
    return text if "." in text else text + ".0"


def format_card(key: str, value: object, comment: str | None = None) -> bytes:
    if isinstance(value, bool):
        field = f"{'T' if value else 'F':>20}"
    elif isinstance(value, int):
        field = f"{value:>20}"
    elif isinstance(value, float):
        field = f"{format_float(value):>20}"
    else:
        text = str(value).replace("'", "''")
        field = f"'{text:<8}'"
    line = f"{key:<8}= {field}"
    if comment:
        line += f" / {comment}"
    if len(line) > 80:
        if 10 + len(field) > 80:
            raise CropError(f"value for {key} does not fit on one card")
        line = line[:80]
    if not line.isascii():
        raise CropError(f"card {key} is not ASCII")
    return line.ljust(80).encode("ascii")


def numeric_value(card: bytes) -> float | None:
    if card[8:10] != b"= ":
        return None
    text = card[10:].decode("ascii", "replace")
    if text.lstrip().startswith("'"):
        return None
    try:
        return float(text.split("/")[0].strip().replace("D", "E").replace("d", "E"))
    except ValueError:
        return None


def numeric_comment(card: bytes) -> str | None:
    text = card[10:].decode("ascii", "replace")
    if "/" not in text:
        return None
    comment = text.split("/", 1)[1].strip()
    return comment or None


def header_cards(raw: bytes) -> tuple[list[bytes], int]:
    """Primary header cards (END excluded) and the header length in bytes."""
    cards = []
    for start in range(0, len(raw), 80):
        card = raw[start : start + 80]
        if len(card) < 80:
            break
        if card[:3] == b"END" and card[3:].strip() == b"":
            header_len = ((start + 80 + 2879) // 2880) * 2880
            return cards, header_len
        cards.append(card)
    raise CropError("FITS header has no END card")


# ----------------------------------------------------------- FITS checksums


def ones_complement_sum(data: bytes) -> int:
    """32-bit ones-complement sum of big-endian words (FITS checksum convention)."""
    padded = data + b"\0" * (-len(data) % 4)
    words = np.frombuffer(padded, dtype=">u4").astype(np.uint64)
    total = int(words.sum(dtype=np.uint64)) if len(words) < (1 << 31) else sum(int(w) for w in words)
    while total >> 32:
        total = (total & 0xFFFFFFFF) + (total >> 32)
    return total


def encode_checksum(value: int) -> str:
    """ASCII encoding of the complemented checksum (Seaman et al., FITS checksum proposal)."""
    value = (~value) & 0xFFFFFFFF
    excluded = set(b":;<=>?@[\\]^_`")
    chars = [0] * 16
    for i, byte in enumerate(value.to_bytes(4, "big")):
        quotient, remainder = byte // 4 + 0x30, byte % 4
        group = [quotient + remainder, quotient, quotient, quotient]  # remainder on the first character
        changed = True
        while changed:
            changed = False
            for k in (0, 2):
                if group[k] in excluded or group[k + 1] in excluded:
                    group[k] += 1
                    group[k + 1] -= 1
                    changed = True
        for j in range(4):
            chars[4 * j + i] = group[j]
    rotated = chars[-1:] + chars[:-1]
    return bytes(rotated).decode("ascii")


# ------------------------------------------------------------------- crop


def read_crop(reference: Path) -> tuple[tuple[int, int, int, int], tuple[int, int], str]:
    """(x0, y0, width, height) zero-based, original (NAXIS1, NAXIS2), science MO_SHA."""
    header = fits.getheader(reference, 0)
    if header.get("MO_CROP") is not True:
        raise CropError(f"{reference}: not a cropped product (MO_CROP = T missing)")
    match = SECTION.fullmatch(str(header.get("MO_SEC", "")))
    if match is None:
        raise CropError(f"{reference}: MO_SEC missing or not of the form [x1:x2,y1:y2]")
    x1, x2, y1, y2 = map(int, match.groups())
    if not (1 <= x1 <= x2 and 1 <= y1 <= y2):
        raise CropError(f"{reference}: MO_SEC {match.group(0)} is not a valid section")
    try:
        original = (int(header["MO_ONAX1"]), int(header["MO_ONAX2"]))
    except KeyError as error:
        raise CropError(f"{reference}: missing {error.args[0]}") from None
    if x2 > original[0] or y2 > original[1]:
        raise CropError(f"{reference}: MO_SEC exceeds MO_ONAX1/MO_ONAX2")
    return (x1 - 1, y1 - 1, x2 - x1 + 1, y2 - y1 + 1), original, str(header.get("MO_SHA", ""))


def crop_frame(
    source: Path,
    destination: Path,
    rect: tuple[int, int, int, int],
    original: tuple[int, int],
    science_sha: str,
) -> None:
    x0, y0, width, height = rect
    raw = source.read_bytes()
    cards, header_len = header_cards(raw)
    header = fits.Header.fromstring(b"".join(cards).decode("ascii", "replace"))
    if header.get("MO_CROP") is True:
        raise CropError(f"{source}: already cropped; refusing to crop again")
    bitpix, naxis = int(header["BITPIX"]), int(header["NAXIS"])
    if naxis != 2:
        raise CropError(f"{source}: expected a 2D primary image")
    shape = (int(header["NAXIS1"]), int(header["NAXIS2"]))
    if shape != original:
        raise CropError(
            f"{source}: size {shape[0]}x{shape[1]} does not match the original "
            f"{original[0]}x{original[1]} of the cropped science frame"
        )
    bpv = abs(bitpix) // 8
    data_len = shape[0] * shape[1] * bpv
    if len(raw) != header_len + ((data_len + 2879) // 2880) * 2880:
        raise CropError(f"{source}: expected a single-HDU file of the declared size")
    for card in cards:
        key = card_key(card)
        value = card[10:].decode("ascii", "replace")
        if UNSUPPORTED_WCS.fullmatch(key) or (key.startswith("CTYPE") and "-TAB" in value.upper()):
            raise CropError(f"{source}: {key} cannot be shifted safely by a crop")
    # Header: verbatim cards with the allowed edits, then the MO_ group.
    out_cards = []
    for card in cards:
        key = card_key(card)
        if key.startswith("MO_") or key in ("CHECKSUM", "DATASUM"):
            continue
        if key in ("NAXIS1", "NAXIS2"):
            card = format_card(key, width if key == "NAXIS1" else height, numeric_comment(card))
        elif (match := SHIFTED.fullmatch(key)) is not None:
            value = numeric_value(card)
            if value is None:
                raise CropError(f"{source}: {key} has no numeric value to shift")
            axis = match.group(1) or match.group(2)
            card = format_card(key, value - (x0 if axis == "1" else y0), numeric_comment(card))
        out_cards.append(card)
    created = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3]
    out_cards += [
        format_card("MO_ACT", "calibration_crop", "same crop as the science product MO_REF"),
        format_card("MO_CROP", True, "T: cropped from the original; see MO_SEC"),
        format_card("MO_SEC", f"[{x0 + 1}:{x0 + width},{y0 + 1}:{y0 + height}]", "kept section of original image (1-based)"),
        format_card("MO_ONAX1", original[0], "NAXIS1 of the original image"),
        format_card("MO_ONAX2", original[1], "NAXIS2 of the original image"),
        format_card("MO_X0", x0, "zero-based crop origin in original image"),
        format_card("MO_Y0", y0, "zero-based crop origin in original image"),
        format_card("MO_UNDF", 0, "samples set undefined by this product"),
        format_card("MO_PROG", PROGRAM, "software that created this product"),
        format_card("MO_DATE", created, "UTC product creation time"),
        format_card("MO_SHA", hashlib.sha256(raw).hexdigest()),
    ]
    if len(source.name) <= 60 and source.name.isascii() and "'" not in source.name:
        out_cards.append(format_card("MO_FILE", source.name))
    if science_sha:
        out_cards.append(format_card("MO_REF", science_sha))
    # Data: the stored bytes of each row segment, unchanged.
    row_bytes, segment = shape[0] * bpv, width * bpv
    body = raw[header_len : header_len + data_len]
    data = b"".join(
        body[(y0 + y) * row_bytes + x0 * bpv : (y0 + y) * row_bytes + x0 * bpv + segment]
        for y in range(height)
    )
    data += b"\0" * (-len(data) % 2880)
    datasum = ones_complement_sum(data)
    out_cards.append(format_card("DATASUM", str(datasum), "data unit checksum"))
    # Conventional CHECKSUM layout (comment slash in column 32); Astropy
    # re-renders this card when verifying, so other layouts fail its check.
    out_cards.append("CHECKSUM= '0000000000000000'   / HDU checksum".ljust(80).encode("ascii"))
    head = b"".join(out_cards) + b"END".ljust(80)
    head += b" " * (-len(head) % 2880)
    total = ones_complement_sum(head) + datasum
    while total >> 32:
        total = (total & 0xFFFFFFFF) + (total >> 32)
    checksum_at = (len(out_cards) - 1) * 80 + 11
    head = head[:checksum_at] + encode_checksum(total).encode("ascii") + head[checksum_at + 16 :]
    product = head + data
    if ones_complement_sum(product) not in (0, 0xFFFFFFFF):
        raise CropError(f"{destination}: checksum invariant failed")
    destination.write_bytes(product)
    verify(source, destination, cards, rect, shape, bpv)


def verify(source: Path, destination: Path, source_cards: list[bytes], rect, shape, bpv) -> None:
    """Re-read the output: checksums, verbatim cards and stored sample bytes."""
    x0, y0, width, height = rect
    written = destination.read_bytes()
    cards, header_len = header_cards(written)

    def kept(card: bytes) -> bool:
        key = card_key(card)
        return not (
            key.startswith("MO_")
            or key in ("NAXIS1", "NAXIS2", "CHECKSUM", "DATASUM")
            or SHIFTED.fullmatch(key)
        )

    if [c for c in source_cards if kept(c)] != [c for c in cards if kept(c)]:
        raise CropError(f"{destination}: original header cards were not preserved verbatim")
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        with fits.open(destination, checksum=True, do_not_scale_image_data=True, uint=False) as check:
            out = np.ascontiguousarray(check[0].data)
    with fits.open(source, do_not_scale_image_data=True, uint=False) as original:
        expected = np.ascontiguousarray(original[0].data[y0 : y0 + height, x0 : x0 + width])
    if out.dtype != expected.dtype or out.tobytes() != expected.tobytes():
        raise CropError(f"{destination}: written samples differ from the source section")


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
    except (CropError, OSError, KeyError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
