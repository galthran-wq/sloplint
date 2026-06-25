#!/usr/bin/env python3
"""Regress SQM metrics against external quality / health / provenance labels (fetch_labels.py).

The popularity study found metrics ⊥ stars once size is controlled. This asks sharper questions
on better labels:
  1. PROVENANCE (the project's central question): do AI-authored repos differ in our SQM metrics,
     size-controlled? (ai_share / claude_share — a covariate, never a quality verdict.)
  2. QUALITY/DISCIPLINE: do metrics track has_ci / engineered / bugfix / review / cadence | LOC?
  3. SUPERVISED: logistic(metric panel) → `engineered`, 5-fold CV AUROC — the slop_index's
     supervised counterpart on an OBJECTIVE label.

Stdlib only; reuses the stats from study.py.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
from collections import defaultdict
from pathlib import Path

import study  # spearman, partial_spearman, auroc, ranks

METRICS = study.METRICS

# Continuous labels to partial against log(LOC). (key, display)
CONT_LABELS = [
    ("ai_share", "ai_share"),
    ("claude_share", "claude_share"),
    ("bugfix_ratio", "bugfix_ratio"),
    ("contributors_log", "log(contributors)"),
    ("recent_authors", "recent_authors"),
    ("merged_prs_log", "log(merged_PRs)"),
    ("releases_log", "log(releases)"),
    ("commits_per_week", "commits/wk"),
    ("active_week_frac", "active_wk_frac"),
    ("reviewed_rate", "reviewed_rate"),
]

# Key metrics shown in the AI-vs-nonAI and per-bucket band views.
KEY_METRICS = ["m.avg_cognitive", "m.max_cognitive", "m.duplication.clone_ratio",
               "tp.test_code_ratio", "tp.assertion_free_rate", "m.top_level_code.avg_ratio",
               "m.god_units.total", "m.max_nesting"]


def load_features(path: Path, min_loc: int) -> dict:
    feats = {}
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        if not r.get("ok") or (r.get("m.total_loc") or 0) < min_loc:
            continue
        r["loc_log"] = math.log1p(r.get("m.total_loc") or 0)
        feats[r["full_name"]] = r
    return feats


def load_labels(path: Path) -> dict:
    out = {}
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        if r.get("ok"):
            out[r["full_name"]] = r
    return out


def med(vals):
    return statistics.median(vals) if vals else float("nan")


def join(feats: dict, labs: dict) -> list[dict]:
    recs = []
    for fn, f in feats.items():
        l = labs.get(fn)
        if not l:
            continue
        r = dict(f)
        for k in ("ai_share", "claude_share", "bugfix_ratio", "recent_authors",
                  "commits_per_week", "active_week_frac", "median_gap_days", "reviewed_rate"):
            r[k] = l.get(k)
        r["has_ci"] = 1 if l.get("has_ci") else 0
        r["engineered"] = 1 if (l.get("has_ci") and (l.get("merged_prs") or 0) > 0
                                and (l.get("releases") or 0) > 0) else 0
        r["contributors_log"] = math.log1p(l["contributors"]) if l.get("contributors") else None
        r["merged_prs_log"] = math.log1p(l.get("merged_prs") or 0)
        r["releases_log"] = math.log1p(l.get("releases") or 0)
        r["ai_authored"] = 1 if ((l.get("ai_share") or 0) > 0 or (l.get("claude_share") or 0) > 0) else 0
        r["ai_heavy"] = 1 if (l.get("ai_share") or 0) >= 0.5 else 0
        r["stratum_b"] = r.get("stratum", "?").split("|")[0]
        recs.append(r)
    return recs


def partial_col(recs, mkey, lkey):
    xs, ys, zs = [], [], []
    for r in recs:
        x, y, z = r.get(mkey), r.get(lkey), r.get("loc_log")
        if None in (x, y, z):
            continue
        xs.append(float(x)); ys.append(float(y)); zs.append(float(z))
    if len(xs) < 30:
        return None, 0
    return study.partial_spearman(xs, ys, zs), len(xs)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    here = Path(__file__).resolve().parent
    ap.add_argument("--features", default=str(here / "features.jsonl"))
    ap.add_argument("--labels", default=str(here / "labels.jsonl"))
    ap.add_argument("--out", default=str(here / "label_report.md"))
    ap.add_argument("--min-loc", type=int, default=50)
    args = ap.parse_args()

    feats = load_features(Path(args.features), args.min_loc)
    labs = load_labels(Path(args.labels))
    # Merge the decoupled contributor-count pass (contributors.jsonl) into the label dict.
    contrib_path = Path(args.labels).with_name("contributors.jsonl")
    if contrib_path.exists():
        for line in contrib_path.read_text().splitlines():
            if not line.strip():
                continue
            c = json.loads(line)
            if c["full_name"] in labs and c.get("contributors") is not None:
                labs[c["full_name"]]["contributors"] = c["contributors"]
    recs = join(feats, labs)
    n = len(recs)
    n_ci = sum(r["has_ci"] for r in recs)
    n_eng = sum(r["engineered"] for r in recs)
    n_ai = sum(r["ai_authored"] for r in recs)
    n_aih = sum(r["ai_heavy"] for r in recs)

    out = [f"# SQM metrics vs external quality / provenance labels\n",
           f"N = {n} (features ∩ labels, loc>={args.min_loc}). "
           f"has_ci={n_ci} ({n_ci/n:.0%}), engineered={n_eng} ({n_eng/n:.0%}), "
           f"ai_authored={n_ai} ({n_ai/n:.0%}), ai_heavy≥50%={n_aih} ({n_aih/n:.0%}).\n",
           "Partial = Spearman(metric, label | log LOC). Provenance (ai/claude) is a covariate, "
           "not a quality verdict.\n"]

    # ---- Section 1: the AI-authorship question -----------------------------
    out.append("\n## 1. Do AI-authored repos differ in SQM metrics? (the central covariate)\n")
    out.append("Partial Spearman of each metric vs ai_share / claude_share, controlling log(LOC):\n")
    out.append("| metric | vs ai_share | vs claude_share |")
    out.append("|---|---|---|")
    for key, label, _ in METRICS:
        pa, na = partial_col(recs, key, "ai_share")
        pc, nc = partial_col(recs, key, "claude_share")
        sa = "·" if pa is None else f"{pa:+.2f}"
        sc = "·" if pc is None else f"{pc:+.2f}"
        out.append(f"| {label} | {sa} | {sc} |")

    out.append("\nAI-authored vs non-AI metric medians, **within each size bucket** (controls size):\n")
    out.append("| bucket | n_ai / n | " + " | ".join(k.split('.')[-1] for k in KEY_METRICS) + " |")
    out.append("|" + "---|" * (len(KEY_METRICS) + 2))
    bys = defaultdict(list)
    for r in recs:
        bys[r["stratum_b"]].append(r)
    for b in ["s0", "s1_9", "s10_49", "s50_199", "s200_999", "s1k_4999", "s5k_up"]:
        rs = bys.get(b, [])
        if not rs:
            continue
        ai = [r for r in rs if r["ai_authored"]]
        non = [r for r in rs if not r["ai_authored"]]
        if len(ai) < 5:
            out.append(f"| {b} | {len(ai)}/{len(rs)} | " + " | ".join("·" for _ in KEY_METRICS) + " |")
            continue
        cells = []
        for k in KEY_METRICS:
            a = med([float(r[k]) for r in ai if r.get(k) is not None])
            o = med([float(r[k]) for r in non if r.get(k) is not None])
            cells.append(f"{a:.2f}/{o:.2f}")
        out.append(f"| {b} | {len(ai)}/{len(rs)} | " + " | ".join(cells) + " |")
    out.append("\n_(cells = AI-authored median / non-AI median; same size bucket → size-matched)_")

    # ---- Section 2: quality-label partials + AUROC -------------------------
    out.append("\n## 2. Metrics vs quality/health labels (partial | log LOC) + AUROC\n")
    out.append("| metric | " + " | ".join(c[1] for c in CONT_LABELS)
               + " | AUROC(has_ci) | AUROC(engineered) |")
    out.append("|" + "---|" * (len(CONT_LABELS) + 3))
    for key, label, _ in METRICS:
        cells = []
        for lkey, _ in CONT_LABELS:
            pr, nn = partial_col(recs, key, lkey)
            cells.append("·" if pr is None else f"{pr:+.2f}")
        for lkey in ("has_ci", "engineered"):
            sc = [float(r[key]) for r in recs if r.get(key) is not None]
            lb = [r[lkey] for r in recs if r.get(key) is not None]
            cells.append(f"{study.auroc(sc, lb):.2f}")
        out.append(f"| {label} | " + " | ".join(cells) + " |")

    # ---- Section 3: size-control sanity for has_ci -------------------------
    out.append("\n## 3. AUROC(has_ci) within each star-bucket (is it more than size?)\n")
    out.append("| bucket | n | ci% | " + " | ".join(k.split('.')[-1] for k in KEY_METRICS[:6]) + " |")
    out.append("|" + "---|" * (6 + 3))
    for b in ["s0", "s1_9", "s10_49", "s50_199", "s200_999", "s1k_4999", "s5k_up"]:
        rs = bys.get(b, [])
        if not rs:
            continue
        nci = sum(r["has_ci"] for r in rs)
        cells = []
        for k in KEY_METRICS[:6]:
            sc = [float(r[k]) for r in rs if r.get(k) is not None]
            lb = [r["has_ci"] for r in rs if r.get(k) is not None]
            a = study.auroc(sc, lb)
            cells.append("·" if math.isnan(a) else f"{a:.2f}")
        out.append(f"| {b} | {len(rs)} | {nci/len(rs):.0%} | " + " | ".join(cells) + " |")

    # ---- Section 4: supervised logistic -> engineered ----------------------
    panel = ["m.avg_cognitive", "m.max_cognitive", "m.duplication.clone_ratio",
             "tp.test_code_ratio", "tp.assertion_free_rate", "m.top_level_code.avg_ratio",
             "m.god_units.total", "m.max_nesting", "m.packages.propagation_cost", "loc_log"]
    X, y = [], []
    for r in recs:
        row = [r.get(k) for k in panel]
        if any(v is None for v in row):
            continue
        X.append([float(v) for v in row]); y.append(r["engineered"])
    if len(X) > 100 and 0 < sum(y) < len(y):
        cv_auc, w = study.logreg_cv(X, y) if hasattr(study, "logreg_cv") else logreg_cv(X, y)
        out.append("\n## 4. Supervised: logistic(metric panel) → `engineered`, 5-fold CV\n")
        out.append(f"- N={len(X)}, positive rate={sum(y)/len(y):.0%}, **CV AUROC = {cv_auc:.3f}**")
        out.append("\n| panel feature | std weight |")
        out.append("|---|---|")
        for k, wj in sorted(zip(panel, w), key=lambda kv: -abs(kv[1])):
            out.append(f"| {k} | {wj:+.2f} |")
    else:
        out.append("\n## 4. Supervised: skipped (insufficient label variance yet)")

    Path(args.out).write_text("\n".join(out) + "\n")
    print(f"wrote {args.out}  N={n} ci={n_ci} eng={n_eng} ai={n_ai}")
    print("\n".join(out))


# Self-contained logistic CV (in case study.py lacks it) -----------------------
def logreg_cv(X, y, folds=5, iters=250, lr=0.1):
    n, p = len(X), len(X[0])
    cols = list(zip(*X))
    stats = [(statistics.mean(c), statistics.pstdev(c) or 1.0) for c in cols]
    Xs = [[(X[i][j] - stats[j][0]) / stats[j][1] for j in range(p)] for i in range(n)]

    def fit(idx):
        w = [0.0] * p; b = 0.0
        for _ in range(iters):
            gw = [0.0] * p; gb = 0.0
            for i in idx:
                z = b + sum(w[j] * Xs[i][j] for j in range(p))
                pr = 1 / (1 + math.exp(-max(min(z, 30), -30)))
                e = pr - y[i]
                for j in range(p):
                    gw[j] += e * Xs[i][j]
                gb += e
            m = len(idx)
            for j in range(p):
                w[j] -= lr * (gw[j] / m + 0.01 * w[j])
            b -= lr * gb / m
        return w, b

    fold_ids = [i % folds for i in range(n)]
    aucs = []
    for f in range(folds):
        tr = [i for i in range(n) if fold_ids[i] != f]
        te = [i for i in range(n) if fold_ids[i] == f]
        if len({y[i] for i in te}) < 2:
            continue
        w, b = fit(tr)
        scores = [b + sum(w[j] * Xs[i][j] for j in range(p)) for i in te]
        aucs.append(study.auroc(scores, [y[i] for i in te]))
    w_full, _ = fit(list(range(n)))
    return (statistics.mean(aucs) if aucs else float("nan")), w_full


if __name__ == "__main__":
    main()
