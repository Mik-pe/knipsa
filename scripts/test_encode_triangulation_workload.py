import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ENCODER = ROOT / "benchmarks/reference/clipper2/encode-triangulation-workload.py"


class TriangulationWorkloadEncoderTests(unittest.TestCase):
    def run_encoder(self, workload):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "workload.json"
            path.write_text(json.dumps(workload), encoding="utf-8")
            return subprocess.run(
                ["python3", str(ENCODER), str(path)],
                text=True,
                capture_output=True,
                check=False,
            )

    def test_preserves_large_integer_coordinates(self):
        coordinate = 4_000_000_000_000_000_001
        result = self.run_encoder({
            "schema": "knipsa-triangulation-workload-v1",
            "coordinate_type": "i64",
            "cases": [{"id": "large", "paths": [[[coordinate, 0], [0, 1], [1, 0]]]}],
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(coordinate), result.stdout.split())

    def test_rejects_non_integer_coordinate_profile(self):
        result = self.run_encoder({
            "schema": "knipsa-triangulation-workload-v1",
            "coordinate_type": "f64",
            "cases": [],
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires i64 coordinates", result.stderr)


if __name__ == "__main__":
    unittest.main()
