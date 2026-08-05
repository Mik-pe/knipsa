#!/usr/bin/env python3
"""Encode the shared JSON workload as a small whitespace protocol."""

import json
import sys


CLIP_TYPES = {"intersection": 0, "union": 1, "difference": 2, "xor": 3}
FILL_RULES = {"even_odd": 0, "non_zero": 1, "positive": 2, "negative": 3}


def main(path):
    with open(path, encoding="utf-8") as stream:
        workload = json.load(stream)
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
                tokens.extend((format(x, ".17g"), format(y, ".17g")))
        tokens.append(str(len(case["clips"])))
        for path_points in case["clips"]:
            tokens.append(str(len(path_points)))
            for x, y in path_points:
                tokens.extend((format(x, ".17g"), format(y, ".17g")))
        print(" ".join(tokens))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: encode-workload.py <workload.json>")
    main(sys.argv[1])
