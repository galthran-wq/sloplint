#!/usr/bin/env python3
"""Visualize the SQM factor structure (axes.py) — scree, loadings heatmap, and the
avg-complexity vs complexity-tail map colored by AI-authorship. Writes PNGs.
"""

from __future__ import annotations

import json
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

import axes as A  # PANEL, load_matrix, residualize, varimax

HERE = Path(__file__).resolve().parent
FACTOR_NAMES = ["avg complexity", "complexity tail", "architecture", "test substance",
                "docs / typing", "module structure", "comments / top-level"]


def load_ai():
    out = {}
    for l in open(HERE / "labels.jsonl"):
        r = json.loads(l)
        if r.get("ok"):
            out[r["full_name"]] = r.get("ai_share") or 0
    return out


def main():
    # rebuild residualized PCA (mirror axes.py)
    keys = [k for k, _ in A.PANEL]
    names = [n for _, n in A.PANEL]
    rows, loc, full = [], [], []
    ai = load_ai()
    for l in open(HERE / "features.jsonl"):
        r = json.loads(l)
        if not r.get("ok") or (r.get("m.total_loc") or 0) < 50:
            continue
        vals = [r.get(k) for k in keys]
        if any(v is None for v in vals):
            continue
        rows.append([float(v) for v in vals]); loc.append(np.log1p(r["m.total_loc"]))
        full.append(r["full_name"])
    X = np.array(rows); loc = np.array(loc)
    R = A.residualize(X, loc)
    Z = (R - R.mean(0)) / R.std(0)
    C = np.corrcoef(Z, rowvar=False)
    vals, vecs = np.linalg.eigh(C)
    o = np.argsort(vals)[::-1]; vals, vecs = vals[o], vecs[:, o]
    k = int((vals >= 1).sum())
    L = vecs[:, :k] * np.sqrt(vals[:k])
    Lr = A.varimax(L)
    order = np.argsort((Lr ** 2).sum(0))[::-1]
    Lr = Lr[:, order]
    # sign-orient each factor so its largest loading is positive (readability)
    for j in range(k):
        if Lr[np.argmax(np.abs(Lr[:, j])), j] < 0:
            Lr[:, j] *= -1

    # ---- 1. scree ----
    fig, ax = plt.subplots(figsize=(8, 4.5))
    n = min(12, len(vals))
    ax.bar(range(1, n + 1), vals[:n], color="#4C72B0", label="eigenvalue")
    ax.axhline(1, color="#C44E52", ls="--", lw=1.2, label="Kaiser cut (=1)")
    ax2 = ax.twinx()
    ax2.plot(range(1, n + 1), np.cumsum(vals[:n]) / vals.sum() * 100, "o-",
             color="#55A868", label="cumulative % var")
    ax.set_xlabel("principal component"); ax.set_ylabel("eigenvalue")
    ax2.set_ylabel("cumulative % variance"); ax2.set_ylim(0, 100)
    ax.set_title(f"Scree — SQM panel has ~{k} independent axes (size-residualized, N={len(X)})\n"
                 "no single dominant component → one scalar index is lossy", fontsize=10)
    ax.legend(loc="upper right", fontsize=8); ax2.legend(loc="center right", fontsize=8)
    fig.tight_layout(); fig.savefig(HERE / "viz_scree.png", dpi=130); plt.close(fig)

    # ---- 2. loadings heatmap ----
    fig, ax = plt.subplots(figsize=(8.5, 7))
    im = ax.imshow(Lr, cmap="RdBu_r", vmin=-1, vmax=1, aspect="auto")
    ax.set_xticks(range(k))
    ax.set_xticklabels([f"F{j+1}\n{FACTOR_NAMES[j] if j < len(FACTOR_NAMES) else ''}"
                        for j in range(k)], fontsize=8)
    ax.set_yticks(range(len(names))); ax.set_yticklabels(names, fontsize=9)
    for i in range(len(names)):
        for j in range(k):
            if abs(Lr[i, j]) >= 0.35:
                ax.text(j, i, f"{Lr[i,j]:.2f}", ha="center", va="center", fontsize=7,
                        color="white" if abs(Lr[i, j]) > 0.6 else "black")
    ax.set_title("What each axis is — varimax-rotated loadings\n"
                 "avg complexity (F1) and complexity tail (F2) are SEPARATE axes", fontsize=10)
    fig.colorbar(im, ax=ax, shrink=0.6, label="loading")
    fig.tight_layout(); fig.savefig(HERE / "viz_loadings.png", dpi=130); plt.close(fig)

    # ---- 3. AI effect per metric: RAW vs size-controlled (what survives) ----
    aivec = np.array([ai.get(f, 0) for f in full])
    heavy = aivec >= 0.5
    nonai = aivec == 0
    Zraw = (X - X.mean(0)) / X.std(0)            # raw standardized
    Zres = (R - R.mean(0)) / R.std(0)            # size-residualized standardized
    raw_eff = np.array([Zraw[heavy, j].mean() - Zraw[nonai, j].mean() for j in range(len(names))])
    res_eff = np.array([Zres[heavy, j].mean() - Zres[nonai, j].mean() for j in range(len(names))])
    o3 = np.argsort(raw_eff)
    y = np.arange(len(names))
    fig, ax = plt.subplots(figsize=(8.5, 7))
    ax.barh(y + 0.2, raw_eff[o3], height=0.4, color="#C44E52", label="RAW (not size-controlled)")
    ax.barh(y - 0.2, res_eff[o3], height=0.4, color="#4C72B0", label="size-controlled (resid. on log LOC)")
    ax.set_yticks(y); ax.set_yticklabels([names[i] for i in o3], fontsize=9)
    ax.axvline(0, color="black", lw=0.8)
    ax.set_xlabel("AI-heavy − non-AI  (mean standardized difference, SD units)")
    ax.set_title("What is actually AI-distinctive? RAW vs size-controlled\n"
                 "max_cog / god_units big when RAW but collapse under size control (AI repos are ~8× larger);\n"
                 "avg complexity, type coverage, low duplication survive — the real signature", fontsize=9)
    ax.legend(loc="lower right", fontsize=9)
    fig.tight_layout(); fig.savefig(HERE / "viz_ai_effect.png", dpi=130); plt.close(fig)

    # ---- 4. avg-complexity vs tail map (honest framing) ----
    def comp(M, cols):
        Zc = (M - M.mean(0)) / M.std(0)
        ix = [names.index(c) for c in cols]
        return Zc[:, ix].mean(1)
    avg_axis = comp(R, ["avg_cog", "avg_cyc", "max_nest"])
    tail_axis = comp(R, ["max_cog", "max_cyc", "god_units"])
    fig, ax = plt.subplots(figsize=(7.5, 7))
    ax.scatter(avg_axis[nonai], tail_axis[nonai], s=4, alpha=0.18, color="#4C72B0",
               label=f"non-AI (n={nonai.sum()})", rasterized=True)
    ax.scatter(avg_axis[heavy], tail_axis[heavy], s=10, alpha=0.7, color="#C44E52",
               label=f"AI-heavy ≥0.5 (n={heavy.sum()})")
    ax.scatter(avg_axis[nonai].mean(), tail_axis[nonai].mean(), s=260, marker="X",
               color="#1F3D6B", edgecolor="white", zorder=5)
    ax.scatter(avg_axis[heavy].mean(), tail_axis[heavy].mean(), s=260, marker="X",
               color="#7A1B22", edgecolor="white", zorder=5)
    ax.axhline(0, color="gray", lw=0.5); ax.axvline(0, color="gray", lw=0.5)
    ax.set_xlabel("← cleaner    AVG complexity (F1)    sloppier →")
    ax.set_ylabel("← cleaner    complexity TAIL (F2)    sloppier →")
    ax.set_xlim(-2, 4); ax.set_ylim(-2, 4)
    ax.set_title("Size-controlled: AI repos shift RIGHT (avg complexity), not UP (tail)\n"
                 "✕ = group mean. The raw 'heavy tail' was mostly size (AI repos ~8× larger).", fontsize=9)
    ax.legend(loc="upper left", fontsize=9)
    fig.tight_layout(); fig.savefig(HERE / "viz_ai_map.png", dpi=130); plt.close(fig)

    print("wrote viz_scree.png, viz_loadings.png, viz_ai_effect.png, viz_ai_map.png")


if __name__ == "__main__":
    main()
