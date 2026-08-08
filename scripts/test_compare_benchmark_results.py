#!/usr/bin/env python3
"""Unit tests for the fail-closed conformance comparator."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("compare-benchmark-results.py")
sys.path.insert(0, str(SCRIPT.parent))
SPEC = importlib.util.spec_from_file_location("compare_benchmark_results", SCRIPT)
assert SPEC and SPEC.loader
COMPARATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARATOR)


def write_jsonl(path: Path, implementation: str, records: list[dict[str, object]]) -> None:
    lines = [{
        "implementation": implementation,
        "samples": 1,
        "warmups": 0,
        "minimum_sample_time_ns": 1,
    }, *records]
    path.write_text("\n".join(json.dumps(line) for line in lines) + "\n", encoding="utf-8")


def record(case_id: str, *, status: str = "ok") -> dict[str, object]:
    return {
        "id": case_id,
        "status": status,
        "error": None if status == "ok" else "reference failure",
        "median_ns": 1,
        "p95_ns": 1,
        "iterations_per_sample": 1,
        "ring_count": 0,
        "signature": "[]",
    }


class ComparatorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.workload = self.root / "workload.json"
        self.workload.write_text(
            json.dumps({
                "schema": "test",
                "coordinate_type": "f64",
                "cases": [{"id": "one"}, {"id": "two"}],
            }),
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        self.directory.cleanup()

    def compare(self, left_records: list[dict[str, object]], right_records: list[dict[str, object]]) -> int:
        left = self.root / "left.jsonl"
        right = self.root / "right.jsonl"
        write_jsonl(left, "left", left_records)
        write_jsonl(right, "right", right_records)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return COMPARATOR.compare(self.workload, left, right)

    def test_complete_matching_outputs_pass(self) -> None:
        self.assertEqual(self.compare([record("one"), record("two")], [record("one"), record("two")]), 0)

    def test_missing_case_fails_closed(self) -> None:
        self.assertNotEqual(self.compare([record("one")], [record("one"), record("two")]), 0)

    def test_adapter_error_is_a_failure(self) -> None:
        self.assertNotEqual(
            self.compare([record("one"), record("two", status="error")], [record("one"), record("two")]),
            0,
        )

    def test_missing_header_is_a_format_error(self) -> None:
        path = self.root / "malformed.jsonl"
        path.write_text(json.dumps(record("one")) + "\n", encoding="utf-8")
        with self.assertRaises(COMPARATOR.ResultFormatError):
            COMPARATOR.load_results(path)

    def test_workload_tolerance_allows_documented_rounding(self) -> None:
        self.workload.write_text(
            json.dumps({
                "schema": "test",
                "coordinate_type": "f64",
                "comparison": {
                    "coordinate_tolerance": 1e-6,
                    "area2_tolerance": 1e-4,
                },
                "cases": [{"id": "one"}],
            }),
            encoding="utf-8",
        )
        left = record("one")
        right = record("one")
        left["signature"] = json.dumps([
            {"depth": 0, "area2": 10.0, "points": [[1.0, 2.0], [3.0, 4.0], [1.0, 5.0]]}
        ])
        left["ring_count"] = 1
        right["signature"] = json.dumps([
            {
                "depth": 0,
                "area2": 10.00001,
                "points": [[1.0000001, 2.0], [3.0, 4.0], [1.0, 5.0]],
            }
        ])
        right["ring_count"] = 1
        self.assertEqual(self.compare([left], [right]), 0)

    def test_invalid_workload_tolerance_fails_closed(self) -> None:
        self.workload.write_text(
            json.dumps({
                "schema": "test",
                "coordinate_type": "f64",
                "comparison": {"coordinate_tolerance": -1},
                "cases": [{"id": "one"}],
            }),
            encoding="utf-8",
        )
        self.assertEqual(self.compare([record("one")], [record("one")]), 2)

    def test_shared_first_vertex_does_not_create_false_nesting(self) -> None:
        right_lobe = [[0, 0], [2, -1], [4, 0], [2, 1]]
        left_lobe = [[0, 0], [-2, -1], [-4, 0], [-2, 1]]
        signature = COMPARATOR.canonical_signature([right_lobe, left_lobe])
        self.assertEqual([ring["depth"] for ring in signature], [0, 0])

    def test_ring_count_must_match_raw_signature(self) -> None:
        left = record("one")
        left["signature"] = json.dumps([[[0, 0], [1, 0], [0, 1]]])
        self.assertNotEqual(
            self.compare([left, record("two")], [record("one"), record("two")]), 0
        )

    def test_out_of_range_signature_fails_closed(self) -> None:
        huge = record("one")
        huge["ring_count"] = 1
        huge["signature"] = json.dumps([[[1e308, 0], [0, 1e308], [-1e308, 0]]])
        self.assertNotEqual(
            self.compare([huge, record("two")], [record("one"), record("two")]), 0
        )

    def test_integer_profile_preserves_coordinates_beyond_f64_precision(self) -> None:
        self.workload.write_text(
            json.dumps({
                "schema": "test",
                "coordinate_type": "i64",
                "comparison": {"coordinate_tolerance": 0, "area2_tolerance": 0},
                "cases": [{"id": "one"}],
            }),
            encoding="utf-8",
        )
        base = 4_000_000_000_000_000_000
        exact = record("one")
        exact["ring_count"] = 1
        exact["signature"] = json.dumps(
            [[[base, base], [base + 100, base], [base + 100, base + 100], [base, base + 100]]]
        )
        same = dict(exact)
        self.assertEqual(self.compare([exact], [same]), 0)

        changed = dict(exact)
        changed["signature"] = json.dumps(
            [[[base, base], [base + 101, base], [base + 100, base + 100], [base, base + 100]]]
        )
        self.assertNotEqual(self.compare([exact], [changed]), 0)

        non_integer = dict(exact)
        non_integer["signature"] = json.dumps(
            [[[float(base), base], [base + 100, base], [base + 100, base + 100], [base, base + 100]]]
        )
        self.assertNotEqual(self.compare([exact], [non_integer]), 0)

    def test_equivalent_self_touching_and_hole_decompositions_match(self) -> None:
        self.workload.write_text(
            json.dumps({
                "schema": "test",
                "coordinate_type": "f64",
                "cases": [{"id": "one"}],
            }),
            encoding="utf-8",
        )
        outer = [[0, 0], [0, 4], [6, 4], [6, 0], [2, 0]]
        hole = [[2, 0], [4, 0], [4, 2], [2, 2]]
        self_touching = outer + hole[1:] + [hole[0]]
        left = record("one")
        right = record("one")
        left["signature"] = json.dumps([
            {"depth": 0, "area2": 40, "points": self_touching}
        ])
        left["ring_count"] = 1
        right["signature"] = json.dumps([
            {"depth": 0, "area2": 48, "points": outer},
            {"depth": 1, "area2": 8, "points": hole},
        ])
        right["ring_count"] = 2
        self.assertEqual(self.compare([left], [right]), 0)

        wrong = record("one")
        wrong["signature"] = json.dumps([
            {"depth": 0, "area2": 48, "points": outer},
            {"depth": 1, "area2": 6, "points": [[2, 0], [3.5, 0], [3.5, 2], [2, 2]]},
        ])
        wrong["ring_count"] = 2
        self.assertNotEqual(self.compare([left], [wrong]), 0)


if __name__ == "__main__":
    unittest.main()
