#!/usr/bin/env python3
"""Does the Alves quality profile (% code in high-risk bands) discriminate against ANY label?

quality_profiles.py showed it doesn't separate engineered vs non. Maybe engineered is a bad label.
Here we test the per-repo profile (% of code volume in the high+very-high cognitive/cyclomatic
bands) against many candidate quality labels — stars, contributors, team size, age, bugfix rate,
AI-share, CI — both raw (tercile split) and size-controlled (within one LOC bucket).

If NO label makes the profile discriminate, that's the honest verdict: complexity-share is
descriptive, not a quality signal. Reuses quality_profiles.py. Stdlib.
"""

from __future__ import annotations

import json
import math
import statistics as st
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

import quality_profiles as Q

STUDY = Q.STUDY
DS = Q.DS
NOW = datetime.now(timezone.utc)
METRICS = [("cognitive", "cognitive"), ("cyclomatic", "cyclomatic"), ("loc", "func LOC")]


def age_days(s):
    if not s:
        return None
    return (NOW - datetime.fromisoformat(s.replace("Z", "+00:00"))).days


def main():
    rloc, eng = Q.repo_loc_and_eng()
    thr = Q.derive_thresholds("functions.jsonl", METRICS, "loc", rloc)

    # per-repo % of code in high+very-high band, per metric (one stream)
    acc = defaultdict(lambda: defaultdict(lambda: [0.0, 0.0]))  # fn->metric->[hi_vh_loc, tot_loc]
    for l in open(DS / "functions.jsonl"):
        r = json.loads(l)
        fn = r.get("full_name")
        if fn not in rloc:
            continue
        w = r.get("loc") or 0
        for m, _ in METRICS:
            v = r.get(m)
            if v is None:
                continue
            a = acc[fn][m]
            a[1] += w
            if Q.band(v, thr[m]) >= 2:
                a[0] += w
    prof = {fn: {m: (mm[m][0] / mm[m][1] * 100 if mm[m][1] else None) for m, _ in METRICS}
            for fn, mm in acc.items()}

    # per-repo labels from repos.jsonl
    meta = {}
    for l in open(DS / "repos.jsonl"):
        r = json.loads(l)
        if not r.get("ok"):
            continue
        meta[r["full_name"]] = {
            "stars": r.get("stars"), "contributors": r.get("contributors"),
            "recent_authors": r.get("recent_authors"), "age": age_days(r.get("created_at")),
            "bugfix": r.get("bugfix_ratio"), "ai_share": r.get("ai_share"),
            "has_ci": 1 if r.get("has_ci") else 0, "engineered": 1 if eng.get(r["full_name"]) else 0,
            "loc": rloc.get(r["full_name"], 0),
        }

    LABELS = [("stars", "stars (hi=more)"), ("contributors", "contributors"),
              ("recent_authors", "team size"), ("age", "age"), ("bugfix", "bugfix rate"),
              ("ai_share", "AI share"), ("has_ci", "has CI"), ("engineered", "engineered")]

    def split(repos, label):
        vals = [(fn, meta[fn][label]) for fn in repos if meta[fn].get(label) is not None]
        if label in ("has_ci", "engineered"):
            hi = [fn for fn, v in vals if v == 1]; lo = [fn for fn, v in vals if v == 0]
        else:
            vs = sorted(v for _, v in vals)
            if len(vs) < 30:
                return [], []
            q1, q2 = vs[len(vs) // 3], vs[2 * len(vs) // 3]
            hi = [fn for fn, v in vals if v >= q2]; lo = [fn for fn, v in vals if v <= q1]
        return hi, lo

    def compare(repos, metric, label):
        hi, lo = split(repos, label)
        h = [prof[fn][metric] for fn in hi if prof.get(fn, {}).get(metric) is not None]
        n = [prof[fn][metric] for fn in lo if prof.get(fn, {}).get(metric) is not None]
        if len(h) < 30 or len(n) < 30:
            return None
        return st.median(h), st.median(n)

    out = ["# Alves profile vs many labels — does % code in high-risk bands discriminate?\n",
           f"Per-repo % of code in high+very-high bands. 'hi' = top tercile of the label (or =1 for "
           "binary), 'lo' = bottom. A discriminating label → lower % for the 'good' side.\n"]
    allr = list(prof.keys())
    mid = [fn for fn in allr if 8000 <= meta[fn]["loc"] < 30000]

    for scope_name, repos in [("ALL repos (raw)", allr), ("size-controlled (8k–30k LOC)", mid)]:
        out.append(f"\n## {scope_name} — N={len(repos)}\n")
        out.append("| label | " + " | ".join(f"{lbl}: hi/lo" for _, lbl in METRICS) + " |")
        out.append("|" + "---|" * (len(METRICS) + 1))
        for lk, llbl in LABELS:
            cells = []
            for mk, _ in METRICS:
                r = compare(repos, mk, lk)
                if r is None:
                    cells.append("·")
                else:
                    h, n = r
                    arrow = "↓" if h < n - 0.5 else ("↑" if h > n + 0.5 else "≈")
                    cells.append(f"{h:.0f}/{n:.0f}{arrow}")
            out.append(f"| {llbl} | " + " | ".join(cells) + " |")
        out.append("\n_(cell = high-group% / low-group%; ↓ = 'good' side has LESS complex code "
                   "(discriminates), ↑ = more, ≈ = flat)_")

    Path(STUDY / "validate_labels_report.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
