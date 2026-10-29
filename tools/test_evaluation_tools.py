#!/usr/bin/env python3
"""Unit tests for flat_labels.py and evaluate_masks.py."""
from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path

import numpy as np
from astropy.io import fits

from tools.evaluate_masks import counts, evaluate, excluded_mask, metrics
from tools.flat_labels import I, Q, U0, V, illumination, labels


class FlatLabelTests(unittest.TestCase):
    def test_illumination_normalises_to_illuminated_level_and_labels_bands(self) -> None:
        flat = np.full((4, 5), 2000.0)
        flat[0, 0] = 100.0   # eta 0.05 -> I
        flat[0, 1] = 600.0   # eta 0.30 -> Q
        flat[0, 2] = 1200.0  # eta 0.60 -> V
        flat[0, 3] = 0.0     # unusable -> I
        flat[0, 4] = np.nan  # unusable -> I
        eta = illumination(flat)
        self.assertAlmostEqual(eta[1, 1], 1.0)
        undefined = np.zeros(flat.shape, dtype=bool)
        undefined[3, 4] = True
        truth = labels(eta, 0.2, 0.5, undefined)
        self.assertEqual(list(truth[0]), [I, Q, V, I, I])
        self.assertEqual(truth[3, 4], U0)
        self.assertEqual(truth[2, 2], V)

    def test_rejects_inverted_thresholds_and_shape_mismatch(self) -> None:
        eta = np.ones((2, 2))
        with self.assertRaises(ValueError):
            labels(eta, 0.6, 0.5)
        with self.assertRaises(ValueError):
            labels(eta, 0.2, 0.5, np.zeros((3, 3), dtype=bool))


class MetricTests(unittest.TestCase):
    def test_counts_and_metrics_exclude_q_and_report_bounds(self) -> None:
        truth = np.array([[I, I, Q, V], [V, V, V, U0]], dtype=np.uint8)
        excluded = np.array([[True, False, True, True], [False, False, False, False]])
        c = counts(truth, excluded)
        self.assertEqual((c["MI"], c["MV"], c["MQ"], c["KI"], c["KV"], c["KQ"]), (1, 1, 1, 1, 3, 0))
        m = metrics(c)
        self.assertAlmostEqual(m["removal_recall"], 0.5)
        self.assertAlmostEqual(m["removal_precision"], 0.5)
        self.assertAlmostEqual(m["valid_loss"], 0.25)
        self.assertAlmostEqual(m["residual_contamination"], 0.25)
        self.assertAlmostEqual(m["valid_loss_worst"], 2 / 5)

    def test_undefined_denominators_are_none_not_perfect(self) -> None:
        truth = np.full((2, 2), V, dtype=np.uint8)
        m = metrics(counts(truth, np.zeros((2, 2), dtype=bool)))
        self.assertIsNone(m["removal_recall"])
        self.assertIsNone(m["removal_precision"])
        self.assertEqual(m["valid_loss"], 0.0)


class ReconstructionTests(unittest.TestCase):
    def test_crop_with_masked_interior_and_kept_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = np.arange(1, 31, dtype=np.float32).reshape(5, 6)
            source[0, 0] = np.nan
            fits.PrimaryHDU(source).writeto(root / "src.fits")
            crop = source[1:4, 2:5].copy()
            crop[0, 0] = np.nan  # interior masked sample
            hdu = fits.PrimaryHDU(crop)
            hdu.header["CLIPX0"] = 2
            hdu.header["CLIPY0"] = 1
            hdu.writeto(root / "crop.fits")
            excluded = excluded_mask(root / "src.fits", root / "crop.fits")
            self.assertFalse(excluded[0, 0])  # originally undefined: not new loss
            self.assertTrue(excluded[1, 2])   # interior masked
            self.assertFalse(excluded[2, 3])  # retained
            self.assertTrue(excluded[4, 5])   # outside rectangle
            self.assertEqual(int(excluded.sum()), 30 - 1 - 8)
            os.symlink(root / "src.fits", root / "kept.fits")
            self.assertFalse(excluded_mask(root / "src.fits", root / "kept.fits").any())

    def test_end_to_end_summary_evaluation_with_groups(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            records = []
            for night in ("n1", "n2", "n3"):
                (root / "in" / night).mkdir(parents=True)
                (root / "truth" / night).mkdir(parents=True)
                (root / "out" / night).mkdir(parents=True)
                data = np.ones((4, 4), dtype=np.float32)
                fits.PrimaryHDU(data).writeto(root / "in" / night / "a.fits")
                truth = np.zeros((4, 4), dtype=np.uint8)
                truth[:, 0] = I
                fits.PrimaryHDU(truth).writeto(root / "truth" / night / "a.fits")
                product = data.copy()
                product[:, 0] = np.nan
                fits.PrimaryHDU(product).writeto(root / "out" / night / "a.fits")
                records.append({"input": f"{night}/a.fits", "output_image": str(root / "out" / night / "a.fits"), "action": "mask"})
            (root / "summary.json").write_text(json.dumps({"files": records, "algorithm_version": "test"}))
            result = evaluate(root / "summary.json", root / "in", root / "truth", 1, 200, 1)
            self.assertEqual(result["images"], 3)
            self.assertEqual(result["pooled"]["removal_recall"], 1.0)
            self.assertEqual(result["pooled"]["valid_loss"], 0.0)
            self.assertEqual(result["bootstrap"]["groups"], 3)


if __name__ == "__main__":
    unittest.main()
