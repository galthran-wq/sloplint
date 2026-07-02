#!/usr/bin/env python3
"""Real Alves/SIG quality profiles on the full dataset (functions.jsonl + classes.jsonl).

Two outputs, the way Alves, Ypma & Visser (2010) / the SIG maintainability model do it:

  1. THRESHOLDS — for each metric, the value at the 70/80/90% quantile of the LOC-weighted,
     per-repo-normalized code-volume distribution across the whole corpus. These define four risk
     bands: low (0–70%), moderate (70–80%), high (80–90%), very-high (>90%). (SIG's quality model.)
  2. QUALITY PROFILES — for each repo, the % of its code volume in each risk band per metric.
     Then validate: do engineered repos carry less code in the high/very-high bands? (If the
     profile is a real quality signal it should discriminate.)

Streams the big tables twice (histogram-based, memory-light). numpy not required. Stdlib.
"""

from __future__ import annotations

import json
import statistics as st
from collections import defaultdict
from pathlib import Path

STUDY = Path(__file__).resolve().parent
DS = STUDY / "dataset"

# (table, value-field, label)
FUNC_METRICS = [("cyclomatic", "unit complexity (cyclomatic)"),
                ("cognitive", "unit complexity (cognitive)"),
                ("loc", "unit size (function LOC)"),
                ("params", "unit interfacing (params)"),
                ("max_nesting", "nesting depth")]
CLASS_METRICS = [("wmc", "WMC"), ("lcom4", "LCOM4"), ("cbo", "CBO"), ("methods", "NOM"), ("dit", "DIT")]
QUANTS = [0.70, 0.80, 0.90]


def repo_loc_and_eng():
    """full_name -> (total_loc, engineered) from repos.jsonl."""
    loc, eng = {}, {}
    for l in open(DS / "repos.jsonl"):
        r = json.loads(l)
        if not r.get("ok"):
            continue
        loc[r["full_name"]] = r.get("m.total_loc") or 0
        eng[r["full_name"]] = bool(r.get("has_ci") and (r.get("merged_prs") or 0) > 0 and (r.get("releases") or 0) > 0)
    return loc, eng


def derive_thresholds(table, metrics, weight_field, rloc):
    """LOC-weighted weighted histograms per metric -> 70/80/90 quantile thresholds."""
    hist = {m: defaultdict(float) for m, _ in metrics}
    for l in open(DS / table):
        r = json.loads(l)
        fn = r.get("full_name")
        tot = rloc.get(fn)
        if not tot:
            continue
        w = (r.get(weight_field) or 0) / tot  # per-repo-normalized LOC weight
        if w <= 0:
            continue
        for m, _ in metrics:
            v = r.get(m)
            if v is not None:
                hist[m][int(v)] += w
    thr = {}
    for m, _ in metrics:
        items = sorted(hist[m].items())
        total = sum(w for _, w in items)
        thr[m] = []
        cum = 0.0
        qi = 0
        for v, w in items:
            cum += w
            while qi < len(QUANTS) and cum / total >= QUANTS[qi]:
                thr[m].append(v)
                qi += 1
        while len(thr[m]) < len(QUANTS):
            thr[m].append(items[-1][0])
    return thr


def band(v, t):
    """risk band index 0..3 given thresholds [q70,q80,q90]."""
    if v <= t[0]:
        return 0
    if v <= t[1]:
        return 1
    if v <= t[2]:
        return 2
    return 3


def profiles(table, metrics, weight_field, thr, rloc, eng):
    """per repo: % of code volume (LOC) in each band per metric; aggregate by engineered."""
    # per-repo accumulators: fn -> metric -> [loc in band0..3]
    acc = defaultdict(lambda: defaultdict(lambda: [0.0, 0.0, 0.0, 0.0]))
    for l in open(DS / table):
        r = json.loads(l)
        fn = r.get("full_name")
        if fn not in rloc:
            continue
        w = r.get(weight_field) or 0
        for m, _ in metrics:
            v = r.get(m)
            if v is not None:
                acc[fn][m][band(v, thr[m])] += w
    # aggregate: distribution of "% in high+very-high" per metric, engineered vs non
    out = {}
    for m, _ in metrics:
        e, n = [], []
        for fn, mm in acc.items():
            bands = mm.get(m)
            if not bands:
                continue
            tot = sum(bands)
            if tot <= 0:
                continue
            pct_hi = (bands[2] + bands[3]) / tot * 100  # high + very-high
            (e if eng.get(fn) else n).append(pct_hi)
        out[m] = (e, n)
    return out


def main():
    rloc, eng = repo_loc_and_eng()
    n_eng = sum(eng.values())
    out = [f"# Alves/SIG quality profiles — full dataset (repos={len(rloc)}, engineered={n_eng})\n",
           "Thresholds = LOC-weighted 70/80/90% quantiles of code volume; bands low/mod/high/very-high.\n"]

    for table, metrics, wf in [("functions.jsonl", FUNC_METRICS, "loc"),
                               ("classes.jsonl", CLASS_METRICS, "loc")]:
        thr = derive_thresholds(table, metrics, wf, rloc)
        out.append(f"\n## {table} — derived thresholds (70 / 80 / 90 %)\n")
        out.append("| metric | low ≤ | moderate ≤ | high ≤ | very-high > |")
        out.append("|---|--:|--:|--:|--:|")
        for m, lbl in metrics:
            t = thr[m]
            out.append(f"| {lbl} | {t[0]} | {t[1]} | {t[2]} | {t[2]} |")

        prof = profiles(table, metrics, wf, thr, rloc, eng)
        out.append(f"\n### Validation — median % of code in high+very-high bands (engineered vs non)\n")
        out.append("| metric | engineered | non-engineered | discriminates? |")
        out.append("|---|--:|--:|---|")
        for m, lbl in metrics:
            e, n = prof[m]
            if len(e) < 30 or len(n) < 30:
                continue
            me, mn = st.median(e), st.median(n)
            tag = "✅ eng cleaner" if me < mn - 0.5 else ("⚠ eng worse" if me > mn + 0.5 else "≈ flat")
            out.append(f"| {lbl} | {me:.1f}% | {mn:.1f}% | {tag} |")

    Path(STUDY / "quality_profiles_report.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
