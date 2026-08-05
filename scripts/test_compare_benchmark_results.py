#!/usr/bin/env python3
"""Unit tests for the fail-closed conformance comparator."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("compare-benchmark-results.py")
SPEC = importlib.util.spec_from_file_location("compare_benchmark_results", SCRIPT)
assert SPEC and SPEC.loader
COMPARATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARATOR)


def write_jsonl(path: Path, implementation: str, records: list[dict[str, object]]) -> None:
    lines = [{"implementation": implementation, "samples": 1, "warmups": 0}, *records]
    path.write_text("\n".join(json.dumps(line) for line in lines) + "\n", encoding="utf-8")


def record(case_id: str, *, status: str = "ok") -> dict[str, object]:
    return {
        "id": case_id,
        "status": status,
        "error": None if status == "ok" else "reference failure",
        "median_ns": 1,
        "p95_ns": 1,
        "ring_count": 0,
        "signature": "[]",
    }


class ComparatorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.workload = self.root / "workload.json"
        self.workload.write_text(
            json.dumps({"schema": "test", "cases": [{"id": "one"}, {"id": "two"}]}),
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


if __name__ == "__main__":
    unittest.main()
