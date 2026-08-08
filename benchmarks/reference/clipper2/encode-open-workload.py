#!/usr/bin/env python3
"""Encode the integer open-path workload as a whitespace protocol."""

import json
import re
import sys


CLIP_TYPES = {"intersection": 0, "union": 1, "difference": 2, "xor": 3}
FILL_RULES = {"even_odd": 0, "non_zero": 1, "positive": 2, "negative": 3}
SAFE_ID = re.compile(r"^[A-Za-z0-9_-]+$")


def coordinate(value):
    if not isinstance(value, int) or isinstance(value, bool):
        raise ValueError(f"i64 workload contains non-integer coordinate {value!r}")
    if not -(1 << 63) <= value < (1 << 63):
        raise ValueError(f"i64 coordinate is out of range: {value}")
    return str(value)


def append_paths(tokens, paths):
    tokens.append(str(len(paths)))
    for path in paths:
        tokens.append(str(len(path)))
        for point in path:
            if not isinstance(point, list) or len(point) != 2:
                raise ValueError(f"invalid point {point!r}")
            tokens.extend(coordinate(value) for value in point)


def main(path):
    with open(path, encoding="utf-8") as stream:
        workload = json.load(stream)
    if workload.get("schema") != "knipsa-open-workload-v1":
        raise ValueError(f"unsupported schema {workload.get('schema')!r}")
    if workload.get("coordinate_type") != "i64":
        raise ValueError("open-path workload must use i64 coordinates")

    for case in workload["cases"]:
        case_id = case["id"]
        if not isinstance(case_id, str) or not SAFE_ID.fullmatch(case_id):
            raise ValueError(f"unsafe case id {case_id!r}")
        tokens = [
            case_id,
            str(CLIP_TYPES[case["clip_type"]]),
            str(FILL_RULES[case["fill_rule"]]),
        ]
        append_paths(tokens, case["closed_subjects"])
        append_paths(tokens, case["open_subjects"])
        append_paths(tokens, case["clips"])
        print(" ".join(tokens))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: encode-open-workload.py <workload.json>")
    main(sys.argv[1])
