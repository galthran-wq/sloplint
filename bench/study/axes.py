#!/usr/bin/env python3
"""How many INDEPENDENT axes does the SQM panel actually have, and what are they?

Answers the question behind "can we make one index, and with what weights?": instead of
hand-weighting, ask the 10k data. Factor structure of the metric panel via PCA:

  - SCREE (eigenvalues / cumulative variance): how many real dimensions, not 18. If one PC
    dominates → a single index is defensible; if variance is spread over k PCs → there are k
    genuinely independent axes and one scalar throws away *which* axis is bad.
  - LOADINGS (varimax-rotated): which metrics group into each axis → the axes get names, and
    the data-driven weights replace guesses.

Run on SIZE-RESIDUALIZED metrics (each regressed on log(LOC), residual taken) so the axes are
the structure of *quality beyond size* — size is the known dominant confound and would otherwise
eat PC1. A second pass keeps size in, to show that explicitly.

numpy only (no sklearn).
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent

# Curated continuous panel. Deliberately keeps the collinear complexity metrics in — collapsing
# them into one factor is exactly what we want to SEE, not assume.
PANEL = [
    ("m.avg_cognitive", "avg_cog"),
    ("m.max_cognitive", "max_cog"),
    ("m.avg_cyclomatic", "avg_cyc"),
    ("m.max_cyclomatic", "max_cyc"),
    ("m.avg_function_loc", "avg_fn_loc"),
    ("m.max_nesting", "max_nest"),
    ("m.god_units.total", "god_units"),
    ("m.duplication.clone_ratio", "clone"),
    ("m.top_level_code.avg_ratio", "toplevel"),
    ("m.comment_density", "comments"),
    ("m.docstring_coverage", "docstr"),
    ("m.param_annotation_coverage", "type_cov"),
    ("m.packages.propagation_cost", "propagation"),
    ("m.packages.cycles.tangles", "cycles"),
    ("m.packages.modularity.gap", "modular_gap"),
    ("tp.test_code_ratio", "test_code"),
    ("tp.assertion_density", "assert_dens"),
    ("tp.assertion_free_rate", "assert_free"),
]


def load_matrix(min_loc: int):
    keys = [k for k, _ in PANEL]
    rows, loc = [], []
    for l in open(HERE / "features.jsonl"):
        r = json.loads(l)
        if not r.get("ok") or (r.get("m.total_loc") or 0) < min_loc:
            continue
        vals = [r.get(k) for k in keys]
        if any(v is None for v in vals):
            continue
        rows.append([float(v) for v in vals])
        loc.append(np.log1p(r["m.total_loc"]))
    return np.array(rows), np.array(loc)


def residualize(X, loc):
    """Regress each column on log(LOC); return residuals (size partialled out)."""
    R = np.empty_like(X)
    A = np.column_stack([np.ones_like(loc), loc])
    for j in range(X.shape[1]):
        beta, *_ = np.linalg.lstsq(A, X[:, j], rcond=None)
        R[:, j] = X[:, j] - A @ beta
    return R


def varimax(L, iters=100, tol=1e-6):
    """Kaiser-normalized varimax rotation of a loadings matrix (p x k)."""
    p, k = L.shape
    if k < 2:
        return L
    h = np.sqrt((L ** 2).sum(axis=1, keepdims=True))
    h[h == 0] = 1
    Ln = L / h
    Rrot = np.eye(k)
    d = 0
    for _ in range(iters):
        Lr = Ln @ Rrot
        u, s, vt = np.linalg.svd(
            Ln.T @ (Lr ** 3 - Lr @ np.diag((Lr ** 2).sum(axis=0)) / p))
        Rrot = u @ vt
        d_old, d = d, s.sum()
        if d_old and d / d_old < 1 + tol:
            break
    return (Ln @ Rrot) * h


def pca_report(X, title, out):
    Z = (X - X.mean(0)) / X.std(0)
    C = np.corrcoef(Z, rowvar=False)
    vals, vecs = np.linalg.eigh(C)
    idx = np.argsort(vals)[::-1]
    vals, vecs = vals[idx], vecs[:, idx]
    total = vals.sum()
    out.append(f"\n## {title}\n")
    out.append("Scree — eigenvalue = # of original metrics' worth of variance a PC captures "
               "(>1 = a real axis, Kaiser):\n")
    out.append("| PC | eigenvalue | % var | cumulative % |")
    out.append("|---|---:|---:|---:|")
    cum = 0
    for i in range(len(vals)):
        cum += vals[i] / total * 100
        mark = " ←Kaiser" if vals[i] >= 1 else ""
        out.append(f"| PC{i+1} | {vals[i]:.2f} | {vals[i]/total*100:.0f}% | {cum:.0f}%{mark} |")
        if i >= 7:
            break
    k = max(2, int((vals >= 1).sum()))
    # loadings = eigvec * sqrt(eigval); varimax-rotate the k retained
    L = vecs[:, :k] * np.sqrt(vals[:k])
    Lr = varimax(L)
    # order factors by variance they carry after rotation
    order = np.argsort((Lr ** 2).sum(0))[::-1]
    Lr = Lr[:, order]
    out.append(f"\n**{k} axes** (Kaiser). Varimax-rotated loadings (|·|≥0.35 shown; sign = direction):\n")
    names = [n for _, n in PANEL]
    header = "| metric | " + " | ".join(f"F{j+1}" for j in range(k)) + " |"
    out.append(header)
    out.append("|" + "---|" * (k + 1))
    for i, nm in enumerate(names):
        cells = []
        for j in range(k):
            v = Lr[i, j]
            cells.append(f"{v:+.2f}" if abs(v) >= 0.35 else "")
        out.append(f"| {nm} | " + " | ".join(cells) + " |")
    # auto-name each factor by its top-loading metrics
    out.append("\nFactor make-up (top metrics by |loading|):")
    for j in range(k):
        top = sorted(range(len(names)), key=lambda i: -abs(Lr[i, j]))[:4]
        desc = ", ".join(f"{names[i]}({Lr[i,j]:+.2f})" for i in top if abs(Lr[i, j]) >= 0.3)
        out.append(f"- **F{j+1}** = {desc}")
    return k


def main():
    X, loc = load_matrix(50)
    out = [f"# Independent axes of the SQM panel — factor structure (N={len(X)})\n",
           f"Panel = {len(PANEL)} continuous metrics. PCA on the standardized panel; the primary "
           "pass residualizes each metric on log(LOC) so the axes are *quality beyond size*.\n"]
    R = residualize(X, loc)
    k1 = pca_report(R, "Size-residualized (the independent quality axes)", out)
    k2 = pca_report(X, "Raw (size kept in — shows size's role)", out)
    out.append("\n## Read\n")
    out.append(f"- The quality panel has **~{k1} independent axes** (size-residualized, Kaiser). "
               "A single scalar index blends them and loses *which* axis is bad — so report the "
               "axis profile; a one-number badge is only an optional rollup of it.")
    out.append("- Each axis' loadings give **data-driven weights** (no hand-guessing): within an "
               "axis, weight metrics by |loading|; across axes, they're ~orthogonal by construction.")
    Path(HERE / "axes_report.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
