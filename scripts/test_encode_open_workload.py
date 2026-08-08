#!/usr/bin/env python3
"""Unit tests for the Clipper2 open-workload encoder."""

import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ENCODER = ROOT / "benchmarks/reference/clipper2/encode-open-workload.py"


class OpenWorkloadEncoderTests(unittest.TestCase):
    def encode(self, *, case_id="case", point=None):
        point = [0, 0] if point is None else point
        workload = {
            "schema": "knipsa-open-workload-v1",
            "coordinate_type": "i64",
            "cases": [{
                "id": case_id,
                "clip_type": "intersection",
                "fill_rule": "even_odd",
                "closed_subjects": [],
                "open_subjects": [[point, [10, 0]]],
                "clips": [[[0, -1], [10, -1], [10, 1], [0, 1]]],
            }],
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "workload.json"
            path.write_text(json.dumps(workload), encoding="utf-8")
            return subprocess.run(
                ["python3", str(ENCODER), str(path)],
                text=True,
                capture_output=True,
                check=False,
            )

    def test_i64_beyond_f64_precision_remains_a_decimal_integer(self):
        coordinate = 4_000_000_000_000_000_001
        result = self.encode(point=[coordinate, coordinate])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split().count(str(coordinate)), 2)

    def test_floating_coordinate_is_rejected(self):
        result = self.encode(point=[1.5, 0])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("non-integer coordinate", result.stderr)

    def test_whitespace_in_case_id_is_rejected(self):
        result = self.encode(case_id="not safe")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsafe case id", result.stderr)


if __name__ == "__main__":
    unittest.main()
