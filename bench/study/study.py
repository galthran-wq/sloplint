#!/usr/bin/env python3
"""Analyse the SQM-metric <-> repo-popularity relationship over the sampled frame.

Consumes features.jsonl (from measure_stream.py; proxies are embedded from the sampling
payload) and writes report.md. The study question (sloplint#55/#142, scaled): do our
software-quality metrics track external repo-health proxies, and which do so *independently
of size* (the known dominant confound)?

For each (metric, proxy) pair we report:
  - Spearman rho (monotone association, outlier-robust),
  - partial Spearman controlling for log(total_loc) — strips the size confound,
  - a bootstrap 95% CI on the partial,
  - AUROC of the metric separating popular (top tercile of stars) from unpopular (bottom),
  - per-star-bucket medians (the band view).

Stdlib only — ranks + Pearson-on-ranks for Spearman, the 3-correlation formula for partial,
Mann-Whitney for AUROC, percentile bootstrap for CIs. No numpy/scipy/pandas dependency.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import statistics
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

STUDY = Path(__file__).resolve().parent
FEATURES = STUDY / "features.jsonl"
REPORT = STUDY / "report.md"

# Metrics we test as candidate quality signals (higher = worse, except where noted). Keys are
# dotted paths in the flattened record; label is the report name; `good_high` marks metrics
# where a HIGHER value is better (tests, annotation) so the sign reads consistently.
METRICS = [
    ("m.avg_cognitive", "avg_cognitive", False),
    ("m.max_cognitive", "max_cognitive", False),
    ("m.avg_cyclomatic", "avg_cyclomatic", False),
    ("m.max_cyclomatic", "max_cyclomatic", False),
    ("m.avg_function_loc", "avg_function_loc", False),
    ("m.max_nesting", "max_nesting", False),
    ("m.god_units.total", "god_units", False),
    ("m.duplication.clone_ratio", "clone_ratio", False),
    ("m.top_level_code.avg_ratio", "toplevel_ratio", False),
    ("m.comment_density", "comment_density", False),
    ("m.docstring_coverage", "docstring_cov", True),
    ("m.param_annotation_coverage", "type_cov", True),
    ("m.packages.propagation_cost", "propagation", False),
    ("m.packages.cycles.tangles", "cycle_tangles", False),
    ("m.packages.modularity.gap", "modularity_gap", False),
    ("tp.test_code_ratio", "test_code_ratio", True),
    ("tp.assertion_density", "assertion_density", True),
    ("tp.assertion_free_rate", "assertion_free_rate", False),
]

PROXIES = [
    ("stars_log", "log(stars+1)"),
    ("forks_log", "log(forks+1)"),
    ("age_days", "age_days"),
    ("recency", "-days_since_push"),
    ("issues_log", "log(open_issues+1)"),
]

NOW = datetime.now(timezone.utc)


def parse_dt(s: str | None) -> datetime | None:
    if not s:
        return None
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def derive_proxies(r: dict) -> None:
    r["stars_log"] = math.log1p(r.get("stars") or 0)
    r["forks_log"] = math.log1p(r.get("forks") or 0)
    r["issues_log"] = math.log1p(r.get("open_issues") or 0)
    created = parse_dt(r.get("created_at"))
    pushed = parse_dt(r.get("pushed_at"))
    r["age_days"] = (NOW - created).days if created else None
    r["recency"] = -((NOW - pushed).days) if pushed else None


# ---- stdlib statistics ----------------------------------------------------

def ranks(xs: list[float]) -> list[float]:
    order = sorted(range(len(xs)), key=lambda i: xs[i])
    rk = [0.0] * len(xs)
    i = 0
    while i < len(xs):
        j = i
        while j + 1 < len(xs) and xs[order[j + 1]] == xs[order[i]]:
            j += 1
        avg = (i + j) / 2 + 1  # 1-based average rank for ties
        for k in range(i, j + 1):
            rk[order[k]] = avg
        i = j + 1
    return rk


def pearson(xs: list[float], ys: list[float]) -> float:
    n = len(xs)
    if n < 3:
        return float("nan")
    mx, my = sum(xs) / n, sum(ys) / n
    sxy = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    sxx = sum((x - mx) ** 2 for x in xs)
    syy = sum((y - my) ** 2 for y in ys)
    if sxx == 0 or syy == 0:
        return float("nan")
    return sxy / math.sqrt(sxx * syy)


def spearman(xs: list[float], ys: list[float]) -> float:
    return pearson(ranks(xs), ranks(ys))


def partial_spearman(xs: list[float], ys: list[float], zs: list[float]) -> float:
    """Spearman partial correlation of x,y controlling z, via the 3-correlation formula
    on ranks."""
    rxy = spearman(xs, ys)
    rxz = spearman(xs, zs)
    ryz = spearman(ys, zs)
    denom = math.sqrt((1 - rxz ** 2) * (1 - ryz ** 2))
    if denom == 0 or any(map(math.isnan, (rxy, rxz, ryz))):
        return float("nan")
    return (rxy - rxz * ryz) / denom


def auroc(scores: list[float], labels: list[int]) -> float:
    """P(score of a positive > score of a negative), ties=0.5. Mann-Whitney form."""
    pos = [s for s, l in zip(scores, labels) if l == 1]
    neg = [s for s, l in zip(scores, labels) if l == 0]
    if not pos or not neg:
        return float("nan")
    rk = ranks(scores)
    rpos = sum(rk[i] for i, l in enumerate(labels) if l == 1)
    return (rpos - len(pos) * (len(pos) + 1) / 2) / (len(pos) * len(neg))


def boot_ci(fn, n_boot: int, seed: int = 1) -> tuple[float, float]:
    rng = random.Random(seed)
    vals = []
    for _ in range(n_boot):
        vals.append(fn(rng))
    vals = [v for v in vals if not math.isnan(v)]
    if not vals:
        return (float("nan"), float("nan"))
    vals.sort()
    lo = vals[int(0.025 * len(vals))]
    hi = vals[min(int(0.975 * len(vals)), len(vals) - 1)]
    return (lo, hi)


# ---- analysis -------------------------------------------------------------

def paired(recs: list[dict], a: str, b: str) -> tuple[list[float], list[float]]:
    xs, ys = [], []
    for r in recs:
        x, y = r.get(a), r.get(b)
        if x is not None and y is not None and not (isinstance(x, float) and math.isnan(x)):
            xs.append(float(x))
            ys.append(float(y))
    return xs, ys


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--features", default=str(FEATURES))
    ap.add_argument("--out", default=str(REPORT))
    ap.add_argument("--boot", type=int, default=1000)
    ap.add_argument("--min-loc", type=int, default=50, help="drop trivially-tiny repos")
    args = ap.parse_args()

    recs = []
    for line in Path(args.features).read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        if not r.get("ok"):
            continue
        if (r.get("m.total_loc") or 0) < args.min_loc:
            continue
        derive_proxies(r)
        r["loc_log"] = math.log1p(r.get("m.total_loc") or 0)
        recs.append(r)

    n = len(recs)
    out = [f"# SQM metrics vs repo-popularity proxies — study report\n",
           f"N = {n} repos (ok, total_loc >= {args.min_loc}). Generated from `features.jsonl`.\n",
           "Partial = Spearman controlling for log(total_loc) (the dominant size confound).\n"]

    # Popularity label for AUROC: top vs bottom tercile of stars.
    star_sorted = sorted(recs, key=lambda r: r.get("stars") or 0)
    t = len(star_sorted) // 3
    lo_set = {id(r) for r in star_sorted[:t]}
    hi_set = {id(r) for r in star_sorted[-t:]}

    # Main table: partial Spearman vs each proxy, + AUROC(stars terciles).
    out.append("\n## Partial Spearman (metric vs proxy | log LOC) + AUROC\n")
    header = "| metric | " + " | ".join(p[1] for p in PROXIES) + " | AUROC(stars) |"
    out.append(header)
    out.append("|" + "---|" * (len(PROXIES) + 2))
    for key, label, good_high in METRICS:
        cells = []
        for pkey, _ in PROXIES:
            xs, ys, zs = [], [], []
            for r in recs:
                x, y, z = r.get(key), r.get(pkey), r.get("loc_log")
                if None in (x, y, z):
                    continue
                xs.append(float(x)); ys.append(float(y)); zs.append(float(z))
            if len(xs) < 10:
                cells.append("·")
                continue
            pr = partial_spearman(xs, ys, zs)
            cells.append("nan" if math.isnan(pr) else f"{pr:+.2f}")
        # AUROC: metric separating top vs bottom star tercile
        sc, lb = [], []
        for r in recs:
            v = r.get(key)
            if v is None:
                continue
            if id(r) in hi_set:
                sc.append(float(v)); lb.append(1)
            elif id(r) in lo_set:
                sc.append(float(v)); lb.append(0)
        a = auroc(sc, lb) if sc else float("nan")
        # orient AUROC so >0.5 means "more popular" consistently regardless of metric polarity
        out.append(f"| {label} | " + " | ".join(cells) + f" | {a:.2f} |")

    # Bootstrap CI on the headline partial: test_code_ratio vs stars | LOC.
    out.append("\n## Headline partials with bootstrap 95% CI (vs log stars | log LOC)\n")
    out.append("| metric | partial rho | 95% CI |")
    out.append("|---|---|---|")
    for key, label, _ in METRICS:
        base = [(float(r[key]), r["stars_log"], r["loc_log"]) for r in recs
                if r.get(key) is not None and r.get("stars_log") is not None]
        if len(base) < 20:
            continue
        pr = partial_spearman([b[0] for b in base], [b[1] for b in base], [b[2] for b in base])

        def one(rng, base=base):
            samp = [base[rng.randrange(len(base))] for _ in range(len(base))]
            return partial_spearman([s[0] for s in samp], [s[1] for s in samp], [s[2] for s in samp])

        lo, hi = boot_ci(one, args.boot)
        flag = " **robust**" if not math.isnan(lo) and (lo > 0) == (hi > 0) and abs(pr) > 0.1 else ""
        out.append(f"| {label} | {pr:+.2f} | [{lo:+.2f}, {hi:+.2f}]{flag} |")

    # Per-star-bucket medians (the band view).
    out.append("\n## Per-star-bucket medians (band view)\n")
    buckets = defaultdict(list)
    for r in recs:
        buckets[r.get("stratum", "?").split("|")[0]].append(r)
    order = ["s0", "s1_9", "s10_49", "s50_199", "s200_999", "s1k_4999", "s5k_up"]
    present = [b for b in order if b in buckets] + [b for b in buckets if b not in order]
    band_metrics = ["m.total_loc", "m.avg_cognitive", "m.max_cognitive", "m.duplication.clone_ratio",
                    "tp.test_code_ratio", "tp.assertion_free_rate", "m.top_level_code.avg_ratio",
                    "m.god_units.total"]
    out.append("| bucket | n | " + " | ".join(b.split(".")[-1] for b in band_metrics) + " |")
    out.append("|" + "---|" * (len(band_metrics) + 2))
    for b in present:
        rs = buckets[b]
        cells = []
        for m in band_metrics:
            vals = [float(r[m]) for r in rs if r.get(m) is not None]
            cells.append(f"{statistics.median(vals):.2f}" if vals else "·")
        out.append(f"| {b} | {len(rs)} | " + " | ".join(cells) + " |")

    Path(args.out).write_text("\n".join(out) + "\n")
    print(f"wrote {args.out} (N={n})")
    print("\n".join(out[:6]))


if __name__ == "__main__":
    main()
