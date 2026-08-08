#!/usr/bin/env python3
"""Unit tests for exact closed/open conformance comparison."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("compare-open-results.py")
sys.path.insert(0, str(SCRIPT.parent))
SPEC = importlib.util.spec_from_file_location("compare_open_results", SCRIPT)
assert SPEC and SPEC.loader
COMPARATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARATOR)


def record(case_id, *, closed=None, open_paths=None, status="ok"):
    closed = [] if closed is None else closed
    open_paths = [] if open_paths is None else open_paths
    return {
        "id": case_id,
        "status": status,
        "error": None if status == "ok" else "adapter failed",
        "median_ns": 10 if status == "ok" else 0,
        "p95_ns": 12 if status == "ok" else 0,
        "iterations_per_sample": 2 if status == "ok" else 0,
        "closed_path_count": len(closed),
        "open_path_count": len(open_paths),
        "closed_signature": json.dumps(closed),
        "open_signature": json.dumps(open_paths),
    }


def write_results(path, implementation, records):
    header = {
        "implementation": implementation,
        "samples": 25,
        "warmups": 3,
        "minimum_sample_time_ns": 2_000_000,
    }
    path.write_text(
        "\n".join(json.dumps(value) for value in [header, *records]) + "\n",
        encoding="utf-8",
    )


class OpenComparatorTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.workload = self.root / "workload.json"
        self.workload.write_text(
            json.dumps({
                "schema": "knipsa-open-workload-v1",
                "coordinate_type": "i64",
                "comparison": {"coordinate_tolerance": 0},
                "cases": [{"id": "one"}],
            }),
            encoding="utf-8",
        )

    def tearDown(self):
        self.directory.cleanup()

    def compare(self, left, right):
        left_path = self.root / "left.jsonl"
        right_path = self.root / "right.jsonl"
        write_results(left_path, "left", left)
        write_results(right_path, "right", right)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return COMPARATOR.compare(self.workload, left_path, right_path)

    def test_open_collinear_subdivision_is_semantically_equal(self):
        left = record("one", open_paths=[[[0, 0], [5, 0], [10, 0]]])
        right = record("one", open_paths=[[[0, 0], [10, 0]]])
        self.assertEqual(self.compare([left], [right]), 0)

    def test_open_direction_is_part_of_the_contract(self):
        forward = record("one", open_paths=[[[0, 0], [10, 0]]])
        reverse = record("one", open_paths=[[[10, 0], [0, 0]]])
        self.assertEqual(self.compare([forward], [reverse]), 1)

    def test_closed_rotation_orientation_and_subdivision_are_ignored(self):
        left = record("one", closed=[[[0, 0], [10, 0], [10, 10], [0, 10]]])
        right = record(
            "one",
            closed=[[[10, 10], [10, 5], [10, 0], [0, 0], [0, 10]]],
        )
        self.assertEqual(self.compare([left], [right]), 0)

    def test_missing_case_fails_closed(self):
        self.assertEqual(self.compare([], [record("one")]), 2)

    def test_adapter_error_is_a_mismatch(self):
        self.assertEqual(self.compare([record("one", status="error")], [record("one")]), 1)

    def test_integer_coordinates_beyond_f64_are_preserved(self):
        base = 4_000_000_000_000_000_000
        exact = record("one", open_paths=[[[base, 0], [base + 1, 0]]])
        changed = record("one", open_paths=[[[base, 0], [base + 2, 0]]])
        self.assertEqual(self.compare([exact], [exact]), 0)
        self.assertEqual(self.compare([exact], [changed]), 1)

    def test_uncalibrated_record_is_a_format_error(self):
        invalid = record("one")
        invalid["iterations_per_sample"] = 0
        left = self.root / "left.jsonl"
        write_results(left, "left", [invalid])
        with self.assertRaises(COMPARATOR.ResultFormatError):
            COMPARATOR.validate_open_results(left)

    def test_duplicate_open_vertex_is_rejected(self):
        invalid = record("one", open_paths=[[[0, 0], [0, 0], [1, 0]]])
        left = self.root / "left.jsonl"
        write_results(left, "left", [invalid])
        with self.assertRaises(COMPARATOR.ResultFormatError):
            COMPARATOR.validate_open_results(left)


if __name__ == "__main__":
    unittest.main()
