#!/usr/bin/env python3
"""Tests for apply_crop.py."""
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import numpy as np
from astropy.io import fits

from tools.apply_crop import main


def reference(path: Path, section: str = "[3:8,2:6]", size: tuple[int, int] = (10, 7)) -> None:
    header = fits.Header()
    header["CROPPED"] = True
    header["CROPSEC"] = section
    header["ORIGNAX1"], header["ORIGNAX2"] = size
    header["ORIGSHA"] = "f" * 64
    fits.PrimaryHDU(np.zeros((5, 6), dtype=np.float32), header=header).writeto(path)


class ApplyCropTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        reference(self.root / "science_cropped.fits")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_tool(self, *calibrations: Path, overwrite: bool = False) -> int:
        args = ["--reference", str(self.root / "science_cropped.fits"), "--output-dir", str(self.root / "out")]
        if overwrite:
            args.append("--overwrite")
        return main(args + [str(path) for path in calibrations])

    def test_unsigned_bias_keeps_raw_samples_and_records_provenance(self) -> None:
        raw = (np.arange(70, dtype=np.int32).reshape(7, 10) * 900 - 32768).astype(np.int16)
        hdu = fits.PrimaryHDU(raw, do_not_scale_image_data=True)
        hdu.header["BZERO"] = 32768
        hdu.header["BSCALE"] = 1
        hdu.header["CRPIX1"], hdu.header["CRPIX2"], hdu.header["CRPIX1A"] = 5.0, 4.0, 2.5
        hdu.header["LTV1"] = -1.0
        bias = self.root / "bias.fits"
        hdu.writeto(bias)
        before = bias.read_bytes()
        self.assertEqual(self.run_tool(bias), 0)
        self.assertEqual(bias.read_bytes(), before)  # input untouched
        out = self.root / "out" / "bias_cropped.fits"
        with fits.open(out, do_not_scale_image_data=True, checksum=True) as hdus:
            header, data = hdus[0].header, hdus[0].data
            self.assertEqual(data.dtype, np.dtype(">i2"))
            self.assertTrue(np.array_equal(data, raw[1:6, 2:8]))
            self.assertEqual(header["BZERO"], 32768)
            self.assertIs(header["CROPPED"], True)
            self.assertEqual(header["CROPSEC"], "[3:8,2:6]")
            self.assertEqual((header["ORIGNAX1"], header["ORIGNAX2"]), (10, 7))
            self.assertEqual((header["CRPIX1"], header["CRPIX2"], header["CRPIX1A"]), (3.0, 3.0, 0.5))
            self.assertEqual((header["LTV1"], header["LTV2"]), (-3.0, -1.0))
            self.assertEqual(header["CROPREF"], "f" * 64)
            self.assertEqual(len(header["ORIGSHA"]), 64)
        # A second run refuses to replace the output without --overwrite.
        self.assertEqual(self.run_tool(bias), 2)
        self.assertEqual(self.run_tool(bias, overwrite=True), 0)

    def test_refuses_size_mismatch_uncropped_reference_and_recropping(self) -> None:
        flat = self.root / "flat.fits"
        fits.PrimaryHDU(np.ones((8, 10), dtype=np.float32)).writeto(flat)
        self.assertEqual(self.run_tool(flat), 2)
        self.assertFalse((self.root / "out" / "flat_cropped.fits").exists())
        plain = self.root / "plain.fits"
        fits.PrimaryHDU(np.ones((7, 10), dtype=np.float32)).writeto(plain)
        self.assertEqual(
            main(["--reference", str(plain), "--output-dir", str(self.root / "o2"), str(plain)]), 2
        )
        done = self.root / "done.fits"
        cropped = fits.PrimaryHDU(np.ones((7, 10), dtype=np.float32))
        cropped.header["CROPPED"] = True
        cropped.writeto(done)
        self.assertEqual(self.run_tool(done), 2)


if __name__ == "__main__":
    unittest.main()
