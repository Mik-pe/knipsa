#!/usr/bin/env python3
"""Encode the integer triangulation JSON workload as a whitespace protocol."""

import json
import sys


def main(path):
    with open(path, encoding="utf-8") as stream:
        workload = json.load(stream)
    if workload.get("schema") != "knipsa-triangulation-workload-v1":
        raise ValueError("unsupported triangulation workload schema")
    if workload.get("coordinate_type") != "i64":
        raise ValueError("Clipper2 triangulation adapter requires i64 coordinates")
    for case in workload["cases"]:
        tokens = [case["id"], str(len(case["paths"]))]
        for path_points in case["paths"]:
            tokens.append(str(len(path_points)))
            for x, y in path_points:
                tokens.extend((str(x), str(y)))
        print(" ".join(tokens))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: encode-triangulation-workload.py <workload.json>")
    main(sys.argv[1])
