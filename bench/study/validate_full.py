#!/usr/bin/env python3
"""Does the mature/collaborative side look BETTER across the WHOLE panel (not just complexity)?

validate_labels.py showed the size-controlled Alves complexity profile discriminates (mature repos
carry less high-complexity code). This extends it to the full panel: per-function complexity, the
per-class CK metrics, and the repo-level aggregates (duplication, tests, docs, types, coupling).

Size-controlled (one LOC bucket). For each metric × label we ask: is the 'good/mature' side (top
tercile of the label) BETTER on that metric, using each metric's known polarity? ✅ yes / ⚠ no / ≈ flat.
Labels: contributors, has_ci, engineered (collaboration/discipline) + stars (the negative control).
Reuses quality_profiles.py. Stdlib.
"""

from __future__ import annotations

import json
import statistics as st
from collections import defaultdict
from pathlib import Path

import quality_profiles as Q

DS = Q.DS
LO, HI = 8000, 30000  # LOC bucket (size control)

FUNC = [("cognitive", "f:cognitive%", "hi"), ("cyclomatic", "f:cyclomatic%", "hi"),
        ("loc", "f:func-size%", "hi"), ("params", "f:params%", "hi"), ("max_nesting", "f:nesting%", "hi")]
CLS = [("wmc", "c:WMC%", "hi"), ("lcom4", "c:LCOM4%", "hi"), ("cbo", "c:CBO%", "hi"), ("methods", "c:NOM%", "hi")]
# repo-level aggregates: (field, label, polarity)  hi=worse-when-high, lo=worse-when-low(=higher better)
AGG = [("m.duplication.clone_ratio", "r:duplication", "hi"),
       ("tp.test_code_ratio", "r:test-code", "lo"),
       ("tp.assertion_free_rate", "r:assert-free", "hi"),
       ("m.docstring_coverage", "r:docstrings", "lo"),
       ("m.param_annotation_coverage", "r:types", "lo"),
       ("m.packages.propagation_cost", "r:propagation", "hi")]
LABELS = ["contributors", "has_ci", "engineered", "stars"]


def main():
    rloc, eng = Q.repo_loc_and_eng()
    # repo meta + aggregates
    meta, agg = {}, {}
    for l in open(DS / "repos.jsonl"):
        r = json.loads(l)
        if not r.get("ok"):
            continue
        fn = r["full_name"]
        meta[fn] = {"contributors": r.get("contributors"), "stars": r.get("stars"),
                    "has_ci": 1 if r.get("has_ci") else 0, "engineered": 1 if eng.get(fn) else 0,
                    "loc": rloc.get(fn, 0)}
        agg[fn] = {a[0]: r.get(a[0]) for a in AGG}

    inb = {fn for fn in meta if LO <= meta[fn]["loc"] < HI}

    # per-function & per-class Alves "% code in high band" profiles (stream)
    def profile(table, metrics, weight="loc", min_entities=0):
        thr = Q.derive_thresholds(table, [(m, _) for m, _, _ in metrics], weight, rloc)
        acc = defaultdict(lambda: defaultdict(lambda: [0.0, 0.0]))
        cnt = defaultdict(int)
        for l in open(DS / table):
            r = json.loads(l)
            fn = r.get("full_name")
            if fn not in inb:
                continue
            cnt[fn] += 1
            w = r.get(weight) or 0
            for m, _, _ in metrics:
                v = r.get(m)
                if v is None:
                    continue
                a = acc[fn][m]; a[1] += w
                if Q.band(v, thr[m]) >= 2:
                    a[0] += w
        prof = {}
        for fn, mm in acc.items():
            if cnt[fn] < min_entities:
                continue
            prof[fn] = {m: (mm[m][0] / mm[m][1] * 100 if mm[m][1] else None) for m, _, _ in metrics}
        return prof

    fprof = profile("functions.jsonl", FUNC)
    cprof = profile("classes.jsonl", CLS, min_entities=5)

    def value(fn, key):
        if key in ("cognitive", "cyclomatic", "loc", "params", "max_nesting"):
            return fprof.get(fn, {}).get(key)
        if key in ("wmc", "lcom4", "cbo", "methods"):
            return cprof.get(fn, {}).get(key)
        return agg.get(fn, {}).get(key)

    def verdict(metric_key, pol, label):
        repos = [fn for fn in inb if value(fn, metric_key) is not None and meta[fn].get(label) is not None]
        if label in ("has_ci", "engineered"):
            hi = [fn for fn in repos if meta[fn][label] == 1]; lo = [fn for fn in repos if meta[fn][label] == 0]
        else:
            vs = sorted(meta[fn][label] for fn in repos)
            if len(vs) < 30:
                return "·"
            a, b = vs[len(vs)//3], vs[2*len(vs)//3]
            hi = [fn for fn in repos if meta[fn][label] >= b]; lo = [fn for fn in repos if meta[fn][label] <= a]
        h = [value(fn, metric_key) for fn in hi]; n = [value(fn, metric_key) for fn in lo]
        h = [x for x in h if x is not None]; n = [x for x in n if x is not None]
        if len(h) < 25 or len(n) < 25:
            return "·"
        mh, mn = st.median(h), st.median(n)
        # 'good side' = hi (more contributors/stars/has_ci/engineered). better?
        better = (mh < mn) if pol == "hi" else (mh > mn)
        margin = abs(mh - mn) > (0.5 if pol != "lo" or "r:" not in metric_key else 0.01)
        mark = "✅" if (better and margin) else ("⚠" if (not better and margin) else "≈")
        return f"{mh:.0f}/{mn:.0f}{mark}" if max(abs(mh), abs(mn)) >= 1 else f"{mh:.2f}/{mn:.2f}{mark}"

    out = [f"# Full-panel validation (size-controlled {LO//1000}–{HI//1000}k LOC, N={len(inb)})\n",
           "✅ = the 'good/mature' side (top tercile of label) is BETTER on this metric; ⚠ = worse; ≈ flat. "
           "cell = good-side / other-side median.\n",
           "| metric | " + " | ".join(LABELS) + " |",
           "|" + "---|" * (len(LABELS) + 1)]
    for key, lbl, pol in FUNC + CLS + AGG:
        out.append(f"| {lbl} | " + " | ".join(verdict(key, pol, L) for L in LABELS) + " |")
    Path(Q.STUDY / "validate_full_report.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
