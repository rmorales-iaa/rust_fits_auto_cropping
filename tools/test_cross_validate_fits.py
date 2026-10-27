"""Independent WCS validation tests for the FITS cross-checker."""

from __future__ import annotations

import unittest
from pathlib import Path
from unittest.mock import patch

import numpy as np
from astropy.io.fits import Header

from tools.cross_validate_fits import verify_wcs


def celestial_header() -> Header:
    header = Header()
    header["NAXIS"] = 2
    header["NAXIS1"] = 96
    header["NAXIS2"] = 96
    cards = {
        "CTYPE1": "RA---TAN",
        "CTYPE2": "DEC--TAN",
        "CRPIX1": 48.5,
        "CRPIX2": 48.5,
        "CRVAL1": 180.0,
        "CRVAL2": 20.0,
        "CDELT1": -0.0002,
        "CDELT2": 0.0002,
        "CTYPE1A": "RA---TAN",
        "CTYPE2A": "DEC--TAN",
        "CRPIX1A": 48.5,
        "CRPIX2A": 48.5,
        "CRVAL1A": 180.01,
        "CRVAL2A": 19.99,
        "CDELT1A": -0.0003,
        "CDELT2A": 0.0003,
        "RADESYSA": "ICRS",
        "WCSNAMEA": "alternate solution",
    }
    for key, value in cards.items():
        header[key] = value
    return header


class VerifyWcsTests(unittest.TestCase):
    def test_primary_and_alternate_wcs_match_after_crop(self) -> None:
        source = celestial_header()
        output = source.copy()
        for suffix in ("", "A"):
            output[f"CRPIX1{suffix}"] -= 7
            output[f"CRPIX2{suffix}"] -= 11
        verify_wcs(source, output, Path("crop.fits"), 7, 11, True)

    def test_zero_origin_crop_still_checks_primary_and_alternate_world_coordinates(self) -> None:
        header = celestial_header()
        verify_wcs(header, header.copy(), Path("zero-origin-crop.fits"), 0, 0, True)

    def test_unshifted_alternate_reference_pixel_is_rejected(self) -> None:
        source = celestial_header()
        output = source.copy()
        output["CRPIX1"] -= 7
        output["CRPIX2"] -= 11
        with self.assertRaisesRegex(AssertionError, r"CRPIX[12]A"):
            verify_wcs(source, output, Path("bad-alternate.fits"), 7, 11, True)

    def test_all_nonfinite_reference_samples_do_not_pass_as_equal_nan(self) -> None:
        class InvalidWcs:
            class AxisInfo:
                lng = 0
                lat = 1

            wcs = AxisInfo()

            def __init__(self, *_args, **_kwargs) -> None:
                pass

            def all_pix2world(self, points: np.ndarray, _origin: int) -> np.ndarray:
                return np.full((len(points), 2), np.nan)

        header = celestial_header()
        with patch("tools.cross_validate_fits.WCS", InvalidWcs):
            with self.assertRaisesRegex(AssertionError, "no finite sampled input coordinates"):
                verify_wcs(header, header.copy(), Path("invalid-wcs.fits"), 0, 0, True)


if __name__ == "__main__":
    unittest.main()
