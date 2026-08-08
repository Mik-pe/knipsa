#!/usr/bin/env python3
"""Encode the shared JSON workload as a small whitespace protocol."""

import json
import sys


CLIP_TYPES = {"intersection": 0, "union": 1, "difference": 2, "xor": 3}
FILL_RULES = {"even_odd": 0, "non_zero": 1, "positive": 2, "negative": 3}


def main(path):
    with open(path, encoding="utf-8") as stream:
        workload = json.load(stream)
    coordinate_type = workload.get("coordinate_type")
    if coordinate_type not in {"f64", "i64"}:
        raise ValueError(f"unsupported coordinate_type {coordinate_type!r}")

    def coordinate(value):
        if coordinate_type == "i64":
            if not isinstance(value, int) or isinstance(value, bool):
                raise ValueError(f"i64 workload contains non-integer coordinate {value!r}")
            if not -(1 << 63) <= value < (1 << 63):
                raise ValueError(f"i64 coordinate is out of range: {value}")
            return str(value)
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise ValueError(f"f64 workload contains non-numeric coordinate {value!r}")
        return format(value, ".17g")

    for case in workload["cases"]:
        tokens = [
            case["id"],
            str(CLIP_TYPES[case["clip_type"]]),
            str(FILL_RULES[case["fill_rule"]]),
            str(len(case["subjects"])),
        ]
        for path_points in case["subjects"]:
            tokens.append(str(len(path_points)))
            for x, y in path_points:
                tokens.extend((coordinate(x), coordinate(y)))
        tokens.append(str(len(case["clips"])))
        for path_points in case["clips"]:
            tokens.append(str(len(path_points)))
            for x, y in path_points:
                tokens.extend((coordinate(x), coordinate(y)))
        print(" ".join(tokens))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: encode-workload.py <workload.json>")
    main(sys.argv[1])
