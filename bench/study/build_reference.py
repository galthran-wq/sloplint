#!/usr/bin/env python3
"""Build reference.json — the size-matched percentile reference for the sloplint badge.

The factor analysis (axes.py) showed the panel has ~7 independent axes and no single dominant
component, so the badge is a per-axis PROFILE, not one scalar. Each axis is a small, interpretable
metric group with a clear good/bad direction (unlike the sign-mixed rotated factors). For each
(LOC bucket × metric) we store percentile breakpoints over the 10k reference, so a new repo's
metric value maps to "percentile vs real Python repos of similar size".

Size is matched by LOC bucket (LOC is the dominant confound — finding 1). Reference cohort is
all measured repos by default; pass --engineered to calibrate against the disciplined subset
(has_ci ∧ merged_prs ∧ releases) for a stricter "vs well-engineered repos your size" reading.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent

# LOC buckets (log-spaced); a repo is matched to peers in its bucket.
LOC_EDGES = [50, 500, 2000, 8000, 30000, 100000, 10**12]

# Interpretable axes (factor-justified independent). polarity: "high"=higher is worse,
# "low"=lower is worse. Multi-metric axes average their per-metric percentiles.
AXES = [
    ("complexity",      ["m.avg_cognitive"],                                   "high"),
    ("complexity_tail", ["m.max_cognitive", "m.god_units.total"],              "high"),
    ("duplication",     ["m.duplication.clone_ratio"],                         "high"),
    ("test_substance",  ["tp.test_code_ratio"],                                "low"),
    ("docs_typing",     ["m.docstring_coverage", "m.param_annotation_coverage"], "low"),
    ("architecture",    ["m.packages.propagation_cost", "m.packages.cycles.tangles"], "high"),
]

ALL_METRICS = sorted({m for _, ms, _ in AXES for m in ms})


def bucket_of(loc: float) -> int:
    for i in range(len(LOC_EDGES) - 1):
        if LOC_EDGES[i] <= loc < LOC_EDGES[i + 1]:
            return i
    return len(LOC_EDGES) - 2


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--features", default=str(HERE / "features.jsonl"))
    ap.add_argument("--labels", default=str(HERE / "labels.jsonl"))
    ap.add_argument("--engineered", action="store_true",
                    help="reference = disciplined subset (has_ci ∧ merged_prs ∧ releases)")
    ap.add_argument("--out", default=str(HERE / "reference.json"))
    args = ap.parse_args()

    eng = set()
    if args.engineered:
        for l in open(args.labels):
            r = json.loads(l)
            if r.get("ok") and r.get("has_ci") and (r.get("merged_prs") or 0) > 0 and (r.get("releases") or 0) > 0:
                eng.add(r["full_name"])

    # collect metric values per (bucket, metric)
    vals = {b: {m: [] for m in ALL_METRICS} for b in range(len(LOC_EDGES) - 1)}
    counts = {b: 0 for b in vals}
    for l in open(args.features):
        r = json.loads(l)
        if not r.get("ok") or (r.get("m.total_loc") or 0) < 50:
            continue
        if args.engineered and r["full_name"] not in eng:
            continue
        b = bucket_of(r["m.total_loc"])
        counts[b] += 1
        for m in ALL_METRICS:
            v = r.get(m)
            if v is not None:
                vals[b][m].append(float(v))

    # store 101 percentile breakpoints per (bucket, metric)
    qs = list(range(101))
    ref = {"loc_edges": LOC_EDGES, "axes": [(n, ms, pol) for n, ms, pol in AXES],
           "cohort": "engineered" if args.engineered else "all",
           "buckets": {}}
    for b in vals:
        ref["buckets"][b] = {"n": counts[b], "metrics": {}}
        for m in ALL_METRICS:
            arr = np.array(vals[b][m]) if vals[b][m] else np.array([0.0])
            ref["buckets"][b]["metrics"][m] = [round(float(x), 6) for x in np.percentile(arr, qs)]

    Path(args.out).write_text(json.dumps(ref))
    print(f"wrote {args.out}  cohort={ref['cohort']}")
    for b in vals:
        lo, hi = LOC_EDGES[b], LOC_EDGES[b + 1]
        print(f"  bucket {b} (LOC {lo}–{hi if hi < 10**11 else '∞'}): n={counts[b]}")


if __name__ == "__main__":
    main()
