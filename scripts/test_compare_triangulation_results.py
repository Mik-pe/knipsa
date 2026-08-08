import importlib.util
import pathlib
import unittest


SCRIPT = pathlib.Path(__file__).with_name("compare-triangulation-results.py")
SPEC = importlib.util.spec_from_file_location("compare_triangulation_results", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ValidateTriangulationTests(unittest.TestCase):
    def setUp(self):
        self.case = {"paths": [[[0, 0], [10, 0], [10, 10], [0, 10]]]}

    @staticmethod
    def record(triangles):
        import json
        return {
            "status": "ok",
            "iterations_per_sample": 1,
            "triangle_count": len(triangles),
            "signature": json.dumps(triangles),
        }

    def test_accepts_valid_partition_with_an_internal_diagonal(self):
        record = self.record([[[0, 0], [10, 0], [10, 10]], [[0, 0], [10, 10], [0, 10]]])
        self.assertTrue(MODULE.validate(self.case, record)[0])

    def test_rejects_area_preserving_wrong_boundary(self):
        record = self.record([[[1, 0], [11, 0], [11, 10]], [[1, 0], [11, 10], [1, 10]]])
        valid, detail = MODULE.validate(self.case, record)
        self.assertFalse(valid)
        self.assertIn("boundary_distance", detail)

    def test_rejects_overlapping_triangles(self):
        record = self.record([[[0, 0], [10, 0], [10, 10]], [[0, 0], [10, 0], [10, 10]]])
        valid, detail = MODULE.validate(self.case, record)
        self.assertFalse(valid)
        self.assertIn("interiors overlap", detail)

    def test_rejects_uncalibrated_record(self):
        record = self.record([[[0, 0], [10, 0], [10, 10]], [[0, 0], [10, 10], [0, 10]]])
        record["iterations_per_sample"] = 0
        self.assertEqual(MODULE.validate(self.case, record), (False, "uncalibrated result"))

    def test_rejects_floating_output_from_integer_profile(self):
        record = self.record([[[0.0, 0], [10, 0], [10, 10]], [[0, 0], [10, 10], [0, 10]]])
        self.assertEqual(
            MODULE.validate(self.case, record),
            (False, "non-integer triangulate64 output"),
        )


if __name__ == "__main__":
    unittest.main()
