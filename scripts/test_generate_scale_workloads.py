import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
GENERATOR = REPO_ROOT / "scripts" / "generate-scale-workloads.py"


class ScaleWorkloadTests(unittest.TestCase):
    def test_generated_profile_is_complete_and_deterministic(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.json"
            second = Path(directory) / "second.json"
            subprocess.run([sys.executable, GENERATOR, first], check=True)
            subprocess.run([sys.executable, GENERATOR, second], check=True)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            workload = json.loads(first.read_text(encoding="utf-8"))

        self.assertEqual(workload["schema"], "knipsa-workload-v1")
        self.assertEqual(len(workload["cases"]), 20)
        identifiers = [case["id"] for case in workload["cases"]]
        self.assertEqual(len(identifiers), len(set(identifiers)))
        self.assertIn("overlap-chain-128-union", identifiers)
        self.assertIn("disjoint-grid-128-union", identifiers)
        self.assertIn("convex-256-intersection", identifiers)
        self.assertIn("convex-256-xor", identifiers)

        for case in workload["cases"]:
            for path in case["subjects"] + case["clips"]:
                self.assertGreaterEqual(len(path), 3)
                self.assertTrue(all(math.isfinite(value) for point in path for value in point))
                doubled_area = sum(
                    point[0] * path[(index + 1) % len(path)][1]
                    - point[1] * path[(index + 1) % len(path)][0]
                    for index, point in enumerate(path)
                )
                self.assertGreater(doubled_area, 0.0)


if __name__ == "__main__":
    unittest.main()
