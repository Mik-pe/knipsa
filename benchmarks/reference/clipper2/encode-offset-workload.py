#!/usr/bin/env python3
"""Encode the shared offset JSON workload as a whitespace protocol."""

import json
import sys


JOINS = {"square": 0, "bevel": 1, "round": 2, "miter": 3}
ENDS = {"polygon": 0, "joined": 1, "butt": 2, "square": 3, "round": 4}


def main(path):
    with open(path, encoding="utf-8") as stream:
        workload = json.load(stream)
    for case in workload["cases"]:
        tokens = [
            case["id"], format(case["delta"], ".17g"),
            str(JOINS[case["join_type"]]), str(ENDS[case["end_type"]]),
            format(case["miter_limit"], ".17g"),
            format(case["arc_tolerance"], ".17g"),
            "1" if case["preserve_collinear"] else "0", str(len(case["paths"])),
        ]
        for path_points in case["paths"]:
            tokens.append(str(len(path_points)))
            for x, y in path_points:
                tokens.extend((format(x, ".17g"), format(y, ".17g")))
        print(" ".join(tokens))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: encode-offset-workload.py <workload.json>")
    main(sys.argv[1])
