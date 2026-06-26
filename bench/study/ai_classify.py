#!/usr/bin/env python3
"""Can a logistic regression on the 7 SQM axes separate AI-heavy from non-AI repos?

Fits logreg with 5-fold CV AUROC on three feature sets, to disentangle the construction
signature from the size confound (AI repos are ~8x larger):

  A. the 7 SIZE-RESIDUALIZED axes (axes.py)  -> can quality-structure alone separate AI?
  B. log(LOC) only                            -> the size baseline (the known big difference)
  C. the 7 axes + log(LOC)                     -> everything together

Label: ai_heavy (ai_share>=0.5) = 1 vs non_ai (ai_share=0) = 0; the ambiguous middle dropped.
Vectorized numpy logreg (L2). AUROC is rank-based (balance-robust).
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np

import axes as A

HERE = Path(__file__).resolve().parent


def auroc(scores, y):
    order = np.argsort(scores)
    ranks = np.empty(len(scores)); ranks[order] = np.arange(1, len(scores) + 1)
    npos = y.sum(); nneg = len(y) - npos
    if npos == 0 or nneg == 0:
        return float("nan")
    return (ranks[y == 1].sum() - npos * (npos + 1) / 2) / (npos * nneg)


def fit_logreg(X, y, l2=1.0, iters=500, lr=0.3):
    """Vectorized logistic regression, gradient descent on standardized X (bias separate)."""
    n, p = X.shape
    w = np.zeros(p); b = 0.0
    for _ in range(iters):
        z = X @ w + b
        pr = 1 / (1 + np.exp(-np.clip(z, -30, 30)))
        e = pr - y
        w -= lr * (X.T @ e / n + l2 / n * w)
        b -= lr * e.mean()
    return w, b


def cv_auroc(X, y, folds=5):
    n = len(y)
    fid = np.arange(n) % folds
    aucs, ws = [], []
    mu, sd = X.mean(0), X.std(0); sd[sd == 0] = 1
    Xs = (X - mu) / sd
    for f in range(folds):
        tr, te = fid != f, fid == f
        if len(np.unique(y[te])) < 2:
            continue
        w, b = fit_logreg(Xs[tr], y[tr])
        aucs.append(auroc(Xs[te] @ w + b, y[te])); ws.append(w)
    w_full, _ = fit_logreg(Xs, y)
    return float(np.mean(aucs)), w_full


def main():
    keys = [k for k, _ in A.PANEL]
    rows, loc, names_full, share = [], [], [], []
    ai = {}
    for l in open(HERE / "labels.jsonl"):
        r = json.loads(l)
        if r.get("ok"):
            ai[r["full_name"]] = r.get("ai_share") or 0
    for l in open(HERE / "features.jsonl"):
        r = json.loads(l)
        if not r.get("ok") or (r.get("m.total_loc") or 0) < 50:
            continue
        v = [r.get(k) for k in keys]
        if any(x is None for x in v) or r["full_name"] not in ai:
            continue
        rows.append([float(x) for x in v]); loc.append(np.log1p(r["m.total_loc"]))
        share.append(ai[r["full_name"]])
    X = np.array(rows); loc = np.array(loc); share = np.array(share)

    # 7 size-residualized factor scores
    R = A.residualize(X, loc)
    Z = (R - R.mean(0)) / R.std(0)
    C = np.corrcoef(Z, rowvar=False)
    vals, vecs = np.linalg.eigh(C)
    o = np.argsort(vals)[::-1]; vals, vecs = vals[o], vecs[:, o]
    k = int((vals >= 1).sum())
    Lr = A.varimax(vecs[:, :k] * np.sqrt(vals[:k]))
    order = np.argsort((Lr ** 2).sum(0))[::-1]; Lr = Lr[:, order]
    for j in range(k):
        if Lr[np.argmax(np.abs(Lr[:, j])), j] < 0:
            Lr[:, j] *= -1
    F = Z @ Lr  # factor scores (N x k)

    # label: heavy vs non, drop the middle
    keep = (share >= 0.5) | (share == 0)
    y = (share[keep] >= 0.5).astype(float)
    Fk, lock = F[keep], loc[keep]
    fac_names = ["F1 avg-complexity", "F2 tail", "F3 architecture", "F4 test-substance",
                 "F5 docs/typing", "F6 module-struct", "F7 comments/toplevel"][:k]

    out = ["# Can logreg on the 7 axes separate AI-heavy vs non-AI?\n",
           f"N={int(keep.sum())} (heavy={int(y.sum())}, non={int((1-y).sum())}); "
           "5-fold CV AUROC. The 7 axes are size-residualized.\n"]
    aA, wA = cv_auroc(Fk, y)
    aB, wB = cv_auroc(lock.reshape(-1, 1), y)
    aC, wC = cv_auroc(np.column_stack([Fk, lock]), y)
    out.append("| feature set | CV AUROC |")
    out.append("|---|---:|")
    out.append(f"| A. 7 size-residualized axes | **{aA:.3f}** |")
    out.append(f"| B. log(LOC) only (size baseline) | **{aB:.3f}** |")
    out.append(f"| C. 7 axes + log(LOC) | **{aC:.3f}** |")
    out.append("\n## Model A coefficients (which axis drives AI-separation, size removed)\n")
    out.append("| axis | std weight |")
    out.append("|---|---:|")
    for nm, wj in sorted(zip(fac_names, wA), key=lambda t: -abs(t[1])):
        out.append(f"| {nm} | {wj:+.2f} |")
    out.append(f"\n**Read:** size alone (B) gives AUROC {aB:.2f}; the size-controlled construction "
               f"axes (A) give {aA:.2f}. Adding size to the axes (C) -> {aC:.2f}. "
               f"{'Size dominates' if aB>aA else 'The axes add real signal'}.")
    Path(HERE / "ai_classify_report.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
