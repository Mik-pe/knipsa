import importlib.util
import json
import pathlib
import unittest


SCRIPT = pathlib.Path(__file__).with_name("run-clipper2-issue-conformance.py")
SPEC = importlib.util.spec_from_file_location("run_clipper2_issue_conformance", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class Clipper2IssueConformanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.cases = json.loads(MODULE.CORPUS.read_text(encoding="utf-8"))["cases"]

    def test_boolean_profile_is_exact_integer_workload(self) -> None:
        workload = MODULE.boolean_workload(self.cases)
        self.assertEqual(workload["schema"], "knipsa-workload-v1")
        self.assertEqual(workload["coordinate_type"], "i64")
        self.assertEqual(workload["comparison"], {"coordinate_tolerance": 0, "area2_tolerance": 0})
        self.assertEqual(len(workload["cases"]), 3)

    def test_open_profile_adds_closed_subjects(self) -> None:
        workload = MODULE.open_workload(self.cases)
        self.assertEqual(workload["schema"], "knipsa-open-workload-v1")
        self.assertEqual(len(workload["cases"]), 2)
        self.assertTrue(all(case["closed_subjects"] == [] for case in workload["cases"]))

    def test_offset_profile_excludes_invariant_only_case(self) -> None:
        workload = MODULE.offset_workload(self.cases)
        identifiers = {case["id"] for case in workload["cases"]}
        self.assertEqual(len(identifiers), 4)
        self.assertNotIn("offset-near-collinear-with-large-shell", identifiers)
        self.assertTrue(all(case["boundary_tolerance"] == 0.05 for case in workload["cases"]))
        self.assertTrue(all(case["area_tolerance"] == 0.2 for case in workload["cases"]))

    def test_triangulation_profile_keeps_huge_origin_case(self) -> None:
        workload = MODULE.triangulation_workload(self.cases)
        self.assertEqual(workload["schema"], "knipsa-triangulation-workload-v1")
        self.assertEqual(
            [case["id"] for case in workload["cases"]],
            ["triangulate-huge-origin-near-collinear-quad"],
        )


if __name__ == "__main__":
    unittest.main()
