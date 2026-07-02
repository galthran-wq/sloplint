#!/usr/bin/env python3
"""Validate the #265 metrics on the FULL rebuilt dataset (all sizes) — reads dataset/, no re-clone.

Confirms whether RFC (and the rest of the new CK + *Qty metrics) discriminate quality across the
whole corpus and within EACH LOC bucket (not just the mid-bucket sample). Per-repo mean of each
metric from functions.jsonl / classes.jsonl; compared across collaboration labels + stars, size-
controlled by reporting per LOC bucket. ✅ good/mature side better · ⚠ worse · ≈ flat. Stdlib.
"""

from __future__ import annotations

import json
import statistics as st
from collections import defaultdict
from pathlib import Path

DS = Path(__file__).resolve().parent / "dataset"
OUT = Path(__file__).resolve().parent / "validate_new_from_dataset_report.md"
LOC_EDGES = [50, 2000, 8000, 30000, 10**12]
CLASS_M = [("rfc", "hi"), ("cbo_modified", "hi"), ("fan_in", "hi"), ("fan_out", "hi"),
           ("nosi", "hi"), ("tcc", "lo"), ("lcc", "lo"), ("lcom_star", "hi")]
FUNC_M = [("loop_qty", "hi"), ("comparisons_qty", "hi"), ("variables_qty", "hi"),
          ("unique_words_qty", "hi"), ("math_ops_qty", "hi")]
LABELS = ["contributors", "has_ci", "engineered", "stars"]


def bucket(loc):
    for i in range(len(LOC_EDGES) - 1):
        if LOC_EDGES[i] <= loc < LOC_EDGES[i + 1]:
            return i
    return len(LOC_EDGES) - 2


def main():
    meta = {}
    for l in open(DS / "repos.jsonl"):
        r = json.loads(l)
        if not r.get("ok"):
            continue
        meta[r["full_name"]] = {
            "loc": r.get("m.total_loc") or 0, "stars": r.get("stars"),
            "contributors": r.get("contributors"), "has_ci": 1 if r.get("has_ci") else 0,
            "engineered": 1 if (r.get("has_ci") and (r.get("merged_prs") or 0) > 0 and (r.get("releases") or 0) > 0) else 0}

    # per-repo mean of each new metric (+ class count)
    csum = defaultdict(lambda: defaultdict(lambda: [0.0, 0]))
    ncls = defaultdict(int)
    for l in open(DS / "classes.jsonl"):
        r = json.loads(l)
        fn = r.get("full_name")
        if fn not in meta:
            continue
        ncls[fn] += 1
        for m, _ in CLASS_M:
            v = r.get(m)
            if v is not None:
                csum[fn][m][0] += v; csum[fn][m][1] += 1
    fsum = defaultdict(lambda: defaultdict(lambda: [0.0, 0]))
    for l in open(DS / "functions.jsonl"):
        r = json.loads(l)
        fn = r.get("full_name")
        if fn not in meta:
            continue
        for m, _ in FUNC_M:
            v = r.get(m)
            if v is not None:
                fsum[fn][m][0] += v; fsum[fn][m][1] += 1
    val = {}
    for fn in meta:
        d = {"_nclasses": ncls.get(fn, 0)}
        for m, _ in CLASS_M:
            s = csum[fn][m]; d[m] = s[0] / s[1] if s[1] else None
        for m, _ in FUNC_M:
            s = fsum[fn][m]; d[m] = s[0] / s[1] if s[1] else None
        val[fn] = d

    def verdict(metric, pol, label, repos, need_cls):
        rs = [fn for fn in repos if val[fn].get(metric) is not None and meta[fn].get(label) is not None
              and (not need_cls or val[fn]["_nclasses"] >= 5)]
        if label in ("has_ci", "engineered"):
            hi = [fn for fn in rs if meta[fn][label] == 1]; lo = [fn for fn in rs if meta[fn][label] == 0]
        else:
            vs = sorted(meta[fn][label] for fn in rs if meta[fn][label] is not None)
            if len(vs) < 30:
                return "·"
            a, b = vs[len(vs)//3], vs[2*len(vs)//3]
            hi = [fn for fn in rs if (meta[fn][label] or -1) >= b]
            lo = [fn for fn in rs if (meta[fn][label] or 1e9) <= a]
        h = [val[fn][metric] for fn in hi]; n = [val[fn][metric] for fn in lo]
        if len(h) < 20 or len(n) < 20:
            return "·"
        mh, mn = st.median(h), st.median(n)
        flat = abs(mh - mn) < 0.05 * max(abs(mh), abs(mn), 1e-9)
        better = (mh < mn) if pol == "hi" else (mh > mn)
        return f"{mh:.2f}/{mn:.2f}{'≈' if flat else ('✅' if better else '⚠')}"

    out = [f"# #265 metrics on FULL dataset (N={len(meta)}) — per LOC bucket\n",
           "Per-repo mean; hi=top tercile of label. ✅ good side better · ⚠ worse · ≈ flat.\n"]
    scopes = [("ALL sizes", list(meta))]
    for b in range(len(LOC_EDGES) - 1):
        scopes.append((f"LOC {LOC_EDGES[b]}–{LOC_EDGES[b+1] if LOC_EDGES[b+1]<10**11 else '∞'}",
                       [fn for fn in meta if bucket(meta[fn]["loc"]) == b]))
    for sname, repos in scopes:
        if len(repos) < 60:
            continue
        out.append(f"\n## {sname} — N={len(repos)}\n")
        out.append("| metric | " + " | ".join(LABELS) + " |")
        out.append("|" + "---|" * (len(LABELS) + 1))
        for m, pol in CLASS_M:
            out.append(f"| {m} ({pol},cls) | " + " | ".join(verdict(m, pol, L, repos, True) for L in LABELS) + " |")
        for m, pol in FUNC_M:
            out.append(f"| {m} ({pol},fn) | " + " | ".join(verdict(m, pol, L, repos, False) for L in LABELS) + " |")
    OUT.write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
