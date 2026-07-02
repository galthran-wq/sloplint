#!/usr/bin/env python3
"""Deep-dive on the AI-authorship signature (the central finding of the 10k study).

label_study.py established that AI-authored repos differ in SQM metrics. This sharpens it:
  A. cohort definitions — non-AI (ai_share=0) vs ai_any (>0) vs ai_heavy (>=0.5), per bucket
  B. ai_heavy vs non-AI size-matched medians (the clean contrast — less dilution than ai_any)
  C. partial Spearman vs ai_share / claude_share | log(LOC), WITH bootstrap 95% CI (robustness)
  D. tail rigor — p50/p90/p95 and fraction-over-threshold for the complexity tail (max_cognitive,
     god_units, max_cyclomatic), ai_heavy vs non-AI, so "heavy tail" isn't just a median artefact
  E. concrete examples — top AI-heavy repos by max_cognitive (grounding)

Provenance is a covariate, never a verdict (slop-is-badness-not-provenance). Stdlib + study.py.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics as S
from collections import defaultdict
from pathlib import Path

import study

HERE = Path(__file__).resolve().parent
BUCKETS = ["s0", "s1_9", "s10_49", "s50_199", "s200_999", "s1k_4999", "s5k_up"]


def load(min_loc: int):
    feats = {}
    for l in open(HERE / "features.jsonl"):
        r = json.loads(l)
        if r.get("ok") and (r.get("m.total_loc") or 0) >= min_loc:
            r["loc_log"] = math.log1p(r["m.total_loc"])
            feats[r["full_name"]] = r
    recs = []
    for l in open(HERE / "labels.jsonl"):
        lr = json.loads(l)
        if not lr.get("ok") or lr["full_name"] not in feats:
            continue
        r = feats[lr["full_name"]]
        r["ai_share"] = lr.get("ai_share")
        r["claude_share"] = lr.get("claude_share")
        ai = lr.get("ai_share") or 0
        r["grp"] = "ai_heavy" if ai >= 0.5 else ("ai_any" if ai > 0 else "non_ai")
        r["bucket"] = r.get("stratum", "?").split("|")[0]
        recs.append(r)
    return recs


def med(rs, k):
    v = [float(r[k]) for r in rs if r.get(k) is not None]
    return S.median(v) if v else float("nan")


def pct(rs, k, q):
    v = sorted(float(r[k]) for r in rs if r.get(k) is not None)
    if not v:
        return float("nan")
    return v[min(int(q * len(v)), len(v) - 1)]


def frac_over(rs, k, thr):
    v = [float(r[k]) for r in rs if r.get(k) is not None]
    return sum(1 for x in v if x > thr) / len(v) if v else float("nan")


def boot_partial(triples, n_boot=1000, seed=1):
    import random
    rng = random.Random(seed)
    pr = study.partial_spearman([t[0] for t in triples], [t[1] for t in triples], [t[2] for t in triples])
    vals = []
    m = len(triples)
    for _ in range(n_boot):
        s = [triples[rng.randrange(m)] for _ in range(m)]
        vals.append(study.partial_spearman([t[0] for t in s], [t[1] for t in s], [t[2] for t in s]))
    vals = sorted(v for v in vals if not math.isnan(v))
    lo = vals[int(0.025 * len(vals))] if vals else float("nan")
    hi = vals[min(int(0.975 * len(vals)), len(vals) - 1)] if vals else float("nan")
    return pr, lo, hi


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--min-loc", type=int, default=50)
    ap.add_argument("--boot", type=int, default=800)
    ap.add_argument("--out", default=str(HERE / "ai_signature_report.md"))
    args = ap.parse_args()
    recs = load(args.min_loc)
    by = defaultdict(lambda: defaultdict(list))
    for r in recs:
        by[r["bucket"]][r["grp"]].append(r)
    grp = defaultdict(list)
    for r in recs:
        grp[r["grp"]].append(r)

    n_non, n_any, n_heavy = len(grp["non_ai"]), len(grp["ai_any"]), len(grp["ai_heavy"])
    out = ["# AI-authorship signature — deep dive\n",
           f"N={len(recs)}. non_ai={n_non}, ai_any(>0)={n_any}, ai_heavy(≥0.5)={n_heavy}. "
           "ai_share = AI-tool commit-trailer fraction of the last 100 commits (lower bound).\n",
           "_Provenance is a covariate, not a quality verdict — the panel measures construction._\n"]

    # A: cohort by bucket
    out.append("\n## A. Cohort sizes by star bucket\n")
    out.append("| bucket | non_ai | ai_any | ai_heavy |")
    out.append("|---|--:|--:|--:|")
    for b in BUCKETS:
        out.append(f"| {b} | {len(by[b]['non_ai'])} | {len(by[b]['ai_any'])} | {len(by[b]['ai_heavy'])} |")

    # B: ai_heavy vs non_ai size-matched medians
    KEYS = [("m.max_cognitive", "max_cog"), ("m.god_units.total", "god_units"),
            ("m.avg_cognitive", "avg_cog"), ("m.duplication.clone_ratio", "clone"),
            ("tp.test_code_ratio", "test_code"), ("m.param_annotation_coverage", "type_cov"),
            ("m.docstring_coverage", "docstr")]
    out.append("\n## B. ai_heavy vs non_ai — size-matched medians (heavy/non per bucket)\n")
    out.append("| bucket | n_heavy | " + " | ".join(k[1] for k in KEYS) + " |")
    out.append("|" + "---|" * (len(KEYS) + 2))
    for b in BUCKETS:
        h, n = by[b]["ai_heavy"], by[b]["non_ai"]
        if len(h) < 5:
            out.append(f"| {b} | {len(h)} | " + " | ".join("·" for _ in KEYS) + " |")
            continue
        cells = [f"{med(h,k):.2f}/{med(n,k):.2f}" for k, _ in KEYS]
        out.append(f"| {b} | {len(h)} | " + " | ".join(cells) + " |")

    # C: partial Spearman + bootstrap CI
    out.append("\n## C. Partial Spearman vs ai_share / claude_share | log(LOC), with bootstrap 95% CI\n")
    out.append("| metric | vs ai_share [95% CI] | vs claude_share [95% CI] |")
    out.append("|---|---|---|")
    for key, label in KEYS + [("m.avg_cyclomatic", "avg_cyc"), ("m.top_level_code.avg_ratio", "toplevel")]:
        row = [label]
        for lk in ("ai_share", "claude_share"):
            tr = [(float(r[key]), float(r[lk]), r["loc_log"]) for r in recs
                  if r.get(key) is not None and r.get(lk) is not None]
            pr, lo, hi = boot_partial(tr, args.boot)
            robust = "**" if (lo > 0) == (hi > 0) and abs(pr) > 0.05 else ""
            row.append(f"{robust}{pr:+.2f}{robust} [{lo:+.2f},{hi:+.2f}]")
        out.append("| " + " | ".join(row) + " |")

    # D: tail rigor — pooled, size-controlled by reporting within mid-buckets
    out.append("\n## D. Complexity-tail rigor — ai_heavy vs non_ai (p50 / p90 / p95, %over)\n")
    out.append("Pooled over the well-measured mid buckets (s10_49…s1k_4999) to control size:\n")
    mid_h = [r for b in ["s10_49", "s50_199", "s200_999", "s1k_4999"] for r in by[b]["ai_heavy"]]
    mid_n = [r for b in ["s10_49", "s50_199", "s200_999", "s1k_4999"] for r in by[b]["non_ai"]]
    out.append(f"(ai_heavy n={len(mid_h)}, non_ai n={len(mid_n)})\n")
    out.append("| metric | grp | p50 | p90 | p95 | %over |")
    out.append("|---|---|--:|--:|--:|--:|")
    for key, label, thr in [("m.max_cognitive", "max_cog", 50), ("m.god_units.total", "god_units", 0),
                            ("m.max_cyclomatic", "max_cyc", 40)]:
        for gname, rs in [("ai_heavy", mid_h), ("non_ai", mid_n)]:
            out.append(f"| {label} (>{thr}) | {gname} | {pct(rs,key,.5):.0f} | {pct(rs,key,.9):.0f} | "
                       f"{pct(rs,key,.95):.0f} | {frac_over(rs,key,thr):.0%} |")

    # E: examples
    out.append("\n## E. Top AI-heavy repos by max_cognitive (grounding)\n")
    heavy = sorted(grp["ai_heavy"], key=lambda r: -(r.get("m.max_cognitive") or 0))[:15]
    out.append("| repo | stars | max_cog | avg_cog | god_units | test_code | LOC |")
    out.append("|---|--:|--:|--:|--:|--:|--:|")
    for r in heavy:
        out.append(f"| {r['full_name']} | {r.get('stars','?')} | {r.get('m.max_cognitive')} | "
                   f"{r.get('m.avg_cognitive'):.1f} | {r.get('m.god_units.total')} | "
                   f"{(r.get('tp.test_code_ratio') or 0):.2f} | {r.get('m.total_loc')} |")

    Path(args.out).write_text("\n".join(out) + "\n")
    print(f"wrote {args.out}  (non_ai={n_non} ai_any={n_any} ai_heavy={n_heavy})")
    print("\n".join(out))


if __name__ == "__main__":
    main()
