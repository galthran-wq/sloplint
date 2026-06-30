#!/usr/bin/env python3
"""Do good repos cluster by SQM into recognizable domains (web / DS / CLI / ...)?

If code-quality norms are domain-relative (a DS script is legitimately more complex / less tested
than a web service), then percentile thresholds should be per-domain, not global — which would fix
the "fastapi looks complex vs random repos" problem. First test: take the engineered cohort (our
scalable proxy for good code) and cluster it in the size-controlled SQM axis space. Are there
natural clusters, and do they look like domains?

KMeans (numpy) on the 7 size-residualized factor scores; silhouette picks k; clusters characterized
by axis profile + top example repos. numpy only.
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np

import axes as A

HERE = Path(__file__).resolve().parent
FAC_NAMES = ["avg-complexity", "tail", "architecture", "test-substance",
             "docs/typing", "module-struct", "comments/toplevel"]


def kmeans(X, k, iters=100, restarts=8, seed=0):
    rng = np.random.default_rng(seed)
    best = None
    for _ in range(restarts):
        # k-means++ init
        c = [X[rng.integers(len(X))]]
        for _ in range(k - 1):
            d = np.min([np.sum((X - ci) ** 2, 1) for ci in c], axis=0)
            p = d / d.sum()
            c.append(X[rng.choice(len(X), p=p)])
        C = np.array(c)
        for _ in range(iters):
            lab = np.argmin(((X[:, None] - C[None]) ** 2).sum(2), axis=1)
            newC = np.array([X[lab == j].mean(0) if (lab == j).any() else C[j] for j in range(k)])
            if np.allclose(newC, C):
                break
            C = newC
        inertia = sum(((X[lab == j] - C[j]) ** 2).sum() for j in range(k))
        if best is None or inertia < best[2]:
            best = (lab, C, inertia)
    return best[0], best[1]


def silhouette(X, lab, sample=1500, seed=0):
    rng = np.random.default_rng(seed)
    idx = rng.choice(len(X), min(sample, len(X)), replace=False)
    Xs, ls = X[idx], lab[idx]
    D = np.sqrt(((Xs[:, None] - Xs[None]) ** 2).sum(2))
    sil = []
    for i in range(len(Xs)):
        same = ls == ls[i]; same[i] = False
        a = D[i, same].mean() if same.any() else 0
        b = min((D[i, ls == j].mean() for j in set(ls) if j != ls[i]), default=0)
        sil.append((b - a) / max(a, b) if max(a, b) > 0 else 0)
    return float(np.mean(sil))


def main():
    keys = [k for k, _ in A.PANEL]
    rows, loc, full, stars = [], [], [], []
    eng = set()
    for l in open(HERE / "labels.jsonl"):
        r = json.loads(l)
        if r.get("ok") and r.get("has_ci") and (r.get("merged_prs") or 0) > 0 and (r.get("releases") or 0) > 0:
            eng.add(r["full_name"])
    for l in open(HERE / "features.jsonl"):
        r = json.loads(l)
        if not r.get("ok") or (r.get("m.total_loc") or 0) < 50 or r["full_name"] not in eng:
            continue
        v = [r.get(k) for k in keys]
        if any(x is None for x in v):
            continue
        rows.append([float(x) for x in v]); loc.append(np.log1p(r["m.total_loc"]))
        full.append(r["full_name"]); stars.append(r.get("stars") or 0)
    X = np.array(rows); loc = np.array(loc); stars = np.array(stars)

    # size-residualized 7 factor scores
    R = A.residualize(X, loc)
    Z = (R - R.mean(0)) / R.std(0)
    C = np.corrcoef(Z, rowvar=False)
    vals, vecs = np.linalg.eigh(C); o = np.argsort(vals)[::-1]; vals, vecs = vals[o], vecs[:, o]
    kf = int((vals >= 1).sum())
    Lr = A.varimax(vecs[:, :kf] * np.sqrt(vals[:kf]))
    order = np.argsort((Lr ** 2).sum(0))[::-1]; Lr = Lr[:, order]
    F = Z @ Lr  # factor scores

    out = [f"# Do good (engineered) repos cluster by SQM into domains?\n",
           f"N={len(X)} engineered repos, clustered on {kf} size-residualized SQM axes.\n",
           "## Natural number of clusters (silhouette — higher=cleaner separation)\n",
           "| k | silhouette |", "|---|---:|"]
    sils = {}
    for k in range(2, 8):
        lab, _ = kmeans(F, k)
        s = silhouette(F, lab)
        sils[k] = s
        out.append(f"| {k} | {s:.3f} |")
    bestk = max(sils, key=sils.get)
    out.append(f"\n**Best k = {bestk}** (silhouette {sils[bestk]:.3f}). "
               f"{'Weak — clusters overlap, little domain structure in SQM' if sils[bestk] < 0.25 else 'Some real cluster structure'}.\n")

    lab, Cc = kmeans(F, bestk)
    out.append(f"## The {bestk} clusters — mean axis profile (z) + top-star examples\n")
    out.append("| cluster | n | " + " | ".join(FAC_NAMES[:kf]) + " | example repos (by stars) |")
    out.append("|" + "---|" * (kf + 3))
    for j in range(bestk):
        m = lab == j
        prof = " | ".join(f"{Cc[j][f]:+.1f}" for f in range(kf))
        ex = [full[i] for i in np.argsort(-stars * m)[:4]]
        out.append(f"| C{j} | {m.sum()} | {prof} | {', '.join(ex)} |")

    Path(HERE / "domains_report.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
