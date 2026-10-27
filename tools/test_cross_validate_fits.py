"""Independent FITS metadata, reason-code and WCS checker tests."""

from __future__ import annotations

import unittest
from pathlib import Path
import tempfile
from unittest.mock import patch

import numpy as np
from astropy.io.fits import Header

from tools.cross_validate_fits import (
    verify_preserved_header,
    verify_passthrough_symlink,
    verify_reason_codes,
    verify_wcs,
)


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

    def test_primary_sip_distortion_and_alternate_cd_matrix_survive_crop(self) -> None:
        source = celestial_header()
        source["CTYPE1"] = "RA---TAN-SIP"
        source["CTYPE2"] = "DEC--TAN-SIP"
        source["A_ORDER"] = 2
        source["B_ORDER"] = 2
        source["A_2_0"] = 1.0e-5
        source["B_0_2"] = -1.0e-5
        for key in ("CDELT1A", "CDELT2A"):
            del source[key]
        source["CD1_1A"] = -0.0003
        source["CD1_2A"] = 0.00001
        source["CD2_1A"] = 0.00002
        source["CD2_2A"] = 0.0003
        output = source.copy()
        for suffix in ("", "A"):
            output[f"CRPIX1{suffix}"] -= 7
            output[f"CRPIX2{suffix}"] -= 11
        verify_wcs(source, output, Path("sip-cd-crop.fits"), 7, 11, True)


class VerifyReasonCodeTests(unittest.TestCase):
    def test_mask_mode_codes_match_exclusion_and_input_undefined_pixels(self) -> None:
        original = np.array([[10.0, 11.0], [np.nan, 13.0]])
        exclusion = np.array([[1, 0], [0, 1]], dtype=np.uint8)
        codes = np.array([[1, 0], [3, 1]], dtype=np.uint8)
        verify_reason_codes(original, Header(), exclusion, codes, False, Path("mask-reasons.fits"))

    def test_crop_mode_separates_artifact_crop_sacrifice_and_undefined(self) -> None:
        original = np.array([[10, 11], [-99, 13]], dtype=np.int16)
        header = Header()
        header["BLANK"] = -99
        exclusion = np.array([[1, 1], [0, 1]], dtype=np.uint8)
        codes = np.array([[1, 2], [3, 1]], dtype=np.uint8)
        verify_reason_codes(original, header, exclusion, codes, True, Path("crop-reasons.fits"))

    def test_reason_check_rejects_a_undefined_pixel_marked_as_crop_loss(self) -> None:
        original = np.array([[10, -99]], dtype=np.int16)
        header = Header()
        header["BLANK"] = -99
        exclusion = np.array([[0, 1]], dtype=np.uint8)
        codes = np.array([[0, 2]], dtype=np.uint8)
        with self.assertRaisesRegex(AssertionError, "code 3"):
            verify_reason_codes(original, header, exclusion, codes, True, Path("bad-reasons.fits"))


class VerifyHeaderPolicyTests(unittest.TestCase):
    def test_mask_policy_allows_only_documented_data_and_unit_card_changes(self) -> None:
        source = Header()
        source["OBJECT"] = "field"
        source["INSTRUME"] = "camera"
        source["BUNIT"] = "adu"
        source["DATAMIN"] = 0
        output = source.copy()
        del output["BUNIT"]
        del output["DATAMIN"]
        verify_preserved_header(source, output, Path("mask.fits"), "mask")

    def test_science_product_must_preserve_physical_unit_card(self) -> None:
        source = Header()
        source["BUNIT"] = "adu"
        output = Header()
        with self.assertRaisesRegex(AssertionError, "BUNIT"):
            verify_preserved_header(source, output, Path("science.fits"), "science")

    def test_all_other_source_metadata_must_be_preserved(self) -> None:
        source = Header()
        source["OBJECT"] = "field"
        output = source.copy()
        output["OBJECT"] = "different field"
        with self.assertRaisesRegex(AssertionError, "OBJECT"):
            verify_preserved_header(source, output, Path("science.fits"), "science")

    def test_unregistered_output_card_is_rejected(self) -> None:
        source = Header()
        output = source.copy()
        output["MYSTERY"] = 42
        with self.assertRaisesRegex(AssertionError, "unexpected output header card MYSTERY"):
            verify_preserved_header(source, output, Path("mask.fits"), "mask")


class VerifyPassThroughTests(unittest.TestCase):
    def test_malformed_input_is_accepted_only_as_symlink_to_same_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "broken.fits"
            source.write_bytes(b"not a valid FITS file")
            images = root / "images"
            images.mkdir()
            output = images / source.name
            output.symlink_to(source)
            verify_passthrough_symlink(source, output)

            other = root / "other.fits"
            other.write_bytes(b"different malformed data")
            output.unlink()
            output.symlink_to(other)
            with self.assertRaisesRegex(AssertionError, "does not target its input"):
                verify_passthrough_symlink(source, output)

    def test_passthrough_product_cannot_be_a_regular_copy(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "unsupported.fits"
            source.write_bytes(b"unsupported")
            output = root / "output.fits"
            output.write_bytes(source.read_bytes())
            with self.assertRaisesRegex(AssertionError, "must be a symlink"):
                verify_passthrough_symlink(source, output)


if __name__ == "__main__":
    unittest.main()
