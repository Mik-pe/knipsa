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

    def test_accepts_internal_diagonal_split_by_a_collinear_polygon_vertex(self):
        case = {
            "paths": [[[-80, -20], [-20, -20], [-20, 40], [-50, 10], [-80, 40]]]
        }
        record = self.record([
            [[-50, 10], [-80, 40], [-80, -20]],
            [[-80, -20], [-20, -20], [-20, 40]],
        ])
        self.assertTrue(MODULE.validate(case, record)[0])

    def test_accepts_integer_partition_near_i64_max(self):
        base = (1 << 63) - 1 - 10_000
        case = {
            "paths": [[
                [base, base],
                [base + 100, base],
                [base + 100, base + 100],
                [base, base + 100],
            ]]
        }
        record = self.record([
            [[base, base], [base + 100, base], [base + 100, base + 100]],
            [[base, base], [base + 100, base + 100], [base, base + 100]],
        ])
        self.assertTrue(MODULE.validate(case, record, "i64")[0])

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

    def test_accepts_scale_normalized_floating_partition(self):
        case = {"paths": [[[0.0, 0.0], [1e-12, 0.0], [1e-12, 1e-12], [0.0, 1e-12]]]}
        record = self.record([
            [[0.0, 0.0], [1e-12, 0.0], [1e-12, 1e-12]],
            [[0.0, 0.0], [1e-12, 1e-12], [0.0, 1e-12]],
        ])
        self.assertTrue(MODULE.validate(case, record, "f64")[0])

    def test_accepts_full_range_floating_partition(self):
        maximum = float.fromhex("0x1.fffffffffffffp+1023")
        case = {
            "paths": [[[maximum, 0.0], [0.0, maximum], [-maximum, 0.0]]]
        }
        record = self.record([[[maximum, 0.0], [0.0, maximum], [-maximum, 0.0]]])
        self.assertTrue(MODULE.validate(case, record, "f64")[0])

    def test_rejects_non_finite_floating_output(self):
        record = self.record([[[0.0, 0.0], [10.0, 0.0], [float("nan"), 10.0]]])
        self.assertEqual(
            MODULE.validate(self.case, record, "f64"),
            (False, "non-finite triangulate_d output"),
        )


if __name__ == "__main__":
    unittest.main()
