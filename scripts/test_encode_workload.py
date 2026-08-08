import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ENCODER = ROOT / "benchmarks/reference/clipper2/encode-workload.py"


class WorkloadEncoderTests(unittest.TestCase):
    def encode(self, coordinate_type: str, point: list[object]) -> subprocess.CompletedProcess[str]:
        workload = {
            "coordinate_type": coordinate_type,
            "cases": [
                {
                    "id": "case",
                    "clip_type": "union",
                    "fill_rule": "even_odd",
                    "subjects": [[point, [0, 1], [1, 0]]],
                    "clips": [],
                }
            ],
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

    def test_i64_coordinates_beyond_f64_precision_remain_decimal_integers(self) -> None:
        coordinate = 4_000_000_000_000_000_001
        result = self.encode("i64", [coordinate, coordinate])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split().count(str(coordinate)), 2)

    def test_i64_profile_rejects_floating_coordinates(self) -> None:
        result = self.encode("i64", [1.5, 0])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("non-integer coordinate", result.stderr)


if __name__ == "__main__":
    unittest.main()
