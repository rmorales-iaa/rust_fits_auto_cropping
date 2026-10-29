#!/usr/bin/env python3
"""Unit and simulation tests for shadow_compare.py and drift_monitor.py."""
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import numpy as np

from tools.drift_monitor import binomial_two_sided, holm, monitor
from tools.shadow_compare import design, estimate


def summary(path: Path, records: list[dict]) -> Path:
    path.write_text(json.dumps({"files": records}))
    return path


class BinomialAndHolmTests(unittest.TestCase):
    def test_exact_binomial_matches_closed_form(self) -> None:
        self.assertAlmostEqual(binomial_two_sided(0, 10, 0.5), 2 / 1024)
        self.assertAlmostEqual(binomial_two_sided(3, 10, 0.5), 352 / 1024)
        self.assertAlmostEqual(binomial_two_sided(5, 10, 0.5), 1.0)
        self.assertEqual(binomial_two_sided(1, 10, 0.0), 0.0)
        self.assertEqual(binomial_two_sided(0, 10, 0.0), 1.0)

    def test_holm_step_down(self) -> None:
        self.assertEqual(holm([0.001, 0.02, 0.04], 0.05), [True, True, True])
        self.assertEqual(holm([0.001, 0.03, 0.04], 0.05), [True, False, False])
        self.assertEqual(holm([0.2, 0.3], 0.05), [False, False])


class DriftMonitorTests(unittest.TestCase):
    def test_flags_only_the_drifted_year_and_event(self) -> None:
        rng = np.random.default_rng(1)
        with tempfile.TemporaryDirectory() as tmp:
            runs = {}
            for year in range(2002, 2008):
                p_mask = 0.60 if year == 2005 else 0.20
                records = []
                for i in range(400):
                    action = "mask" if rng.random() < p_mask else "keep"
                    records.append({"input": f"{i}.fits", "status": "kept", "action": action})
                runs[str(year)] = summary(Path(tmp) / f"{year}.json", records)
            result = monitor(runs, 0.05)
            flagged = {(t["year"], t["event"]) for t in result["tests"] if t["flagged"]}
            self.assertIn(("2005", "action_mask"), flagged)
            self.assertIn(("2005", "action_keep"), flagged)
            self.assertTrue(all(year == "2005" for year, _ in flagged))

    def test_homogeneous_years_keep_the_family_wise_error_rate(self) -> None:
        reps, any_flag = 60, 0
        with tempfile.TemporaryDirectory() as tmp:
            for rep in range(reps):
                rng = np.random.default_rng(100 + rep)
                runs = {}
                for year in range(2002, 2010):
                    records = [
                        {"input": f"{i}.fits", "status": "kept", "action": "mask" if rng.random() < 0.2 else "keep"}
                        for i in range(300)
                    ]
                    runs[str(year)] = summary(Path(tmp) / f"{year}.json", records)
                any_flag += any(t["flagged"] for t in monitor(runs, 0.05)["tests"])
        # Expected <= 5%; allow binomial slack for 60 replications.
        self.assertLessEqual(any_flag, 8)

    def test_needs_two_years(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ValueError):
                monitor({"2002": summary(Path(tmp) / "a.json", [])}, 0.05)


def population(n: int, seed: int):
    """Inputs with proposals for A and B plus per-image truth counts."""
    rng = np.random.default_rng(seed)
    a_files, b_files, eval_a, eval_b = [], [], [], []
    for i in range(n):
        name = f"night{i % 20}/f{i}.fits"
        artifact = rng.random() < 0.3
        valid = int(rng.integers(40_000, 60_000))
        invalid = int(rng.integers(2_000, 9_000)) if artifact else 0
        pa = artifact and rng.random() < 0.95
        pb = artifact and rng.random() < 0.70 or (not artifact and rng.random() < 0.05)
        mi_a = int(invalid * rng.uniform(0.9, 1.0)) if pa else 0
        mv_a = int(rng.integers(0, 60)) if pa else 0
        mi_b = int(invalid * rng.uniform(0.5, 0.9)) if pb and artifact else 0
        mv_b = int(rng.integers(100, 2_000)) if pb else 0
        a_files.append({"input": name, "status": "clipped" if pa else "kept", "proposed_mask_pixels": mi_a + mv_a})
        b_files.append({"input": name, "status": "clipped" if pb else "kept", "proposed_mask_pixels": mi_b + mv_b})
        eval_a.append({"input": name, "MI": mi_a, "MV": mv_a, "KI": invalid - mi_a, "KV": valid - mv_a})
        eval_b.append({"input": name, "MI": mi_b, "MV": mv_b, "KI": invalid - mi_b, "KV": valid - mv_b})
    return a_files, b_files, eval_a, eval_b


class ShadowCompareTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        a_files, b_files, eval_a, eval_b = population(1200, 7)
        self.sa = summary(root / "a.json", a_files)
        self.sb = summary(root / "b.json", b_files)
        self.ea = root / "ea.json"
        self.eb = root / "eb.json"
        self.ea.write_text(json.dumps({"per_image": eval_a}))
        self.eb.write_text(json.dumps({"per_image": eval_b}))
        self.true_loss_a = sum(r["MV"] for r in eval_a) / sum(r["MV"] + r["KV"] for r in eval_a)
        self.true_loss_b = sum(r["MV"] for r in eval_b) / sum(r["MV"] + r["KV"] for r in eval_b)

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_design_is_seeded_and_records_inclusion_probabilities(self) -> None:
        sizes = {"agree_keep": 30, "agree_act": 30, "extent": 30, "disagree": 60}
        first = design(self.sa, self.sb, sizes, 11, 2.0)
        again = design(self.sa, self.sb, sizes, 11, 2.0)
        self.assertEqual(first["sample"], again["sample"])
        self.assertEqual(sum(v["N"] for v in first["strata"].values()), 1200)
        for h, v in first["strata"].items():
            self.assertEqual(v["n"], min(sizes[h], v["N"]))
            self.assertAlmostEqual(v["pi"], v["n"] / v["N"] if v["N"] else 0.0)

    def test_census_reproduces_population_ratio_with_zero_variance(self) -> None:
        doc = design(self.sa, self.sb, {h: 10_000 for h in ("agree_keep", "agree_act", "extent", "disagree")}, 3, 2.0)
        result = estimate(doc, self.ea, self.eb)
        loss = result["metrics"]["valid_loss"]
        self.assertAlmostEqual(loss["a"], self.true_loss_a, places=12)
        self.assertAlmostEqual(loss["b"], self.true_loss_b, places=12)
        self.assertAlmostEqual(loss["a_se"], 0.0, places=12)

    def test_enriched_design_is_unbiased_with_nominal_coverage(self) -> None:
        sizes = {"agree_keep": 40, "agree_act": 40, "extent": 40, "disagree": 80}
        estimates, covered = [], 0
        reps = 300
        truth = self.true_loss_a - self.true_loss_b
        for seed in range(reps):
            m = estimate(design(self.sa, self.sb, sizes, seed, 2.0), self.ea, self.eb)["metrics"]["valid_loss"]
            estimates.append(m["difference"])
            lo, hi = m["difference_ci95"]
            covered += lo <= truth <= hi
        bias = np.mean(estimates) - truth
        self.assertLess(abs(bias), 3 * np.std(estimates) / np.sqrt(reps) + 1e-12)
        self.assertGreater(covered / reps, 0.90)
        self.assertLess(covered / reps, 0.99)

    def test_estimate_refuses_an_unsampled_stratum(self) -> None:
        doc = design(self.sa, self.sb, {"agree_keep": 10, "agree_act": 10, "extent": 10}, 1, 2.0)
        with self.assertRaises(ValueError):
            estimate(doc, self.ea, self.eb)


if __name__ == "__main__":
    unittest.main()
