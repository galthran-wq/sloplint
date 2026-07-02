#!/usr/bin/env python3
"""sloplint structural badge — size-matched percentile profile across the ~independent axes.

Scores a repo's metrics against reference.json (the 10k size-matched distribution). Output is a
PROFILE, not one number (the factor analysis showed the axes are independent), with a headline =
the single worst axis. Higher percentile = worse than that fraction of size-matched real repos.

Input (one of):
  --from NAME       pull a repo's metrics from features.jsonl by full_name (demo, no re-clone)
  --metrics FILE    a `sloplint metrics --format json` output file
  --repo PATH       run sloplint on a checkout

Outputs: a text card (stdout), a radar PNG, and a shields-style SVG badge.
"""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
BIN = "/disk1/sloplint/target/release/sloplint"
CFG = str(HERE.parent / "profiles.toml")


def flatten(prefix, obj, out):
    for k, v in obj.items():
        key = f"{prefix}{k}"
        if isinstance(v, dict):
            flatten(key + ".", v, out)
        elif isinstance(v, (int, float, bool)) or v is None:
            out[key] = v


def metrics_from_json(data: dict) -> dict:
    prod = data.get("profiles", {}).get("production", {})
    tp = data.get("test_proxies", {})
    rec = {}
    flatten("m.", prod, rec)
    flatten("tp.", tp, rec)
    return rec


def load_input(args) -> tuple[str, dict]:
    if args.from_name:
        for l in open(HERE / "features.jsonl"):
            r = json.loads(l)
            if r.get("full_name") == args.from_name and r.get("ok"):
                return args.from_name, r
        raise SystemExit(f"{args.from_name} not found in features.jsonl")
    if args.metrics:
        return Path(args.metrics).stem, metrics_from_json(json.loads(Path(args.metrics).read_text()))
    if args.repo:
        out = subprocess.run([BIN, "metrics", args.repo, "--config", CFG, "--scope", "production",
                              "--format", "json"], capture_output=True, text=True)
        return Path(args.repo).name, metrics_from_json(json.loads(out.stdout))
    raise SystemExit("need --from / --metrics / --repo")


def pctile(breakpoints, v) -> float:
    """percentile (0-100) of v within the 101 breakpoints (bp[q] = q-th pct of reference)."""
    import bisect
    return float(bisect.bisect_right(breakpoints, v) - 1)


def score(rec: dict, ref: dict) -> tuple[int, list[dict]]:
    loc = rec.get("m.total_loc") or 0
    edges = ref["loc_edges"]
    b = next((i for i in range(len(edges) - 1) if edges[i] <= loc < edges[i + 1]), len(edges) - 2)
    bucket = ref["buckets"][str(b)]
    axes = []
    for name, ms, pol in ref["axes"]:
        ps = []
        for m in ms:
            v = rec.get(m)
            if v is None:
                continue
            p = pctile(bucket["metrics"][m], float(v))
            ps.append(p if pol == "high" else 100 - p)
        if ps:
            axes.append({"axis": name, "pct": round(sum(ps) / len(ps)), "value": rec.get(ms[0])})
    worst = max(axes, key=lambda a: a["pct"]) if axes else {"axis": "?", "pct": 0}
    return b, axes, worst


def color(p):
    return "#e05d44" if p >= 85 else "#dfb317" if p >= 60 else "#4c1"


def text_card(name, b, edges, n, axes, worst):
    lo, hi = edges[b], edges[b + 1]
    band = f"LOC {lo}–{hi if hi < 10**11 else '∞'}"
    lines = [f"\nsloplint structural profile — {name}",
             f"  vs {n} size-matched repos ({band})\n",
             f"  {'axis':16} {'percentile':>10}   (higher = worse than that % of peers)"]
    for a in sorted(axes, key=lambda x: -x["pct"]):
        bar = "█" * round(a["pct"] / 5)
        flag = "  ⚠" if a["pct"] >= 85 else ""
        lines.append(f"  {a['axis']:16} {('p%02d' % a['pct']):>10}   {bar}{flag}")
    lines.append(f"\n  headline: {worst['axis']} p{worst['pct']:02d}")
    return "\n".join(lines)


def make_svg(worst, path):
    label, val = "sloplint", f"{worst['axis']} p{worst['pct']:02d}"
    c = color(worst["pct"])
    lw, vw = 58, 8 * len(val) + 16
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{lw+vw}" height="20">
<rect rx="3" width="{lw+vw}" height="20" fill="#555"/>
<rect rx="3" x="{lw}" width="{vw}" height="20" fill="{c}"/>
<g fill="#fff" font-family="DejaVu Sans,Verdana,sans-serif" font-size="11">
<text x="6" y="14">{label}</text><text x="{lw+6}" y="14">{val}</text></g></svg>'''
    Path(path).write_text(svg)


def make_radar(name, axes, path):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np
    labels = [a["axis"] for a in axes]
    vals = [a["pct"] for a in axes]
    ang = np.linspace(0, 2 * np.pi, len(labels), endpoint=False).tolist()
    vals2 = vals + vals[:1]; ang2 = ang + ang[:1]
    fig, ax = plt.subplots(figsize=(5.5, 5.5), subplot_kw=dict(polar=True))
    ax.plot(ang2, vals2, color="#e05d44", lw=2)
    ax.fill(ang2, vals2, color="#e05d44", alpha=0.25)
    ax.set_xticks(ang); ax.set_xticklabels(labels, fontsize=9)
    ax.set_ylim(0, 100); ax.set_yticks([25, 50, 75, 100])
    ax.set_yticklabels(["p25", "p50", "p75", "p100"], fontsize=7, color="gray")
    ax.set_title(f"{name} — structural profile (outer = worse vs size-matched peers)", fontsize=10, pad=18)
    fig.tight_layout(); fig.savefig(path, dpi=130); plt.close(fig)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--from", dest="from_name")
    ap.add_argument("--metrics")
    ap.add_argument("--repo")
    ap.add_argument("--ref", default=str(HERE / "reference.json"))
    ap.add_argument("--png")
    ap.add_argument("--svg")
    args = ap.parse_args()
    ref = json.loads(Path(args.ref).read_text())
    name, rec = load_input(args)
    b, axes, worst = score(rec, ref)
    n = ref["buckets"][str(b)]["n"]
    print(text_card(name, b, ref["loc_edges"], n, axes, worst))
    safe = name.replace("/", "_")
    png = args.png or str(HERE / f"badge_{safe}.png")
    svg = args.svg or str(HERE / f"badge_{safe}.svg")
    make_radar(name, axes, png); make_svg(worst, svg)
    print(f"\n  wrote {png}  {svg}")


if __name__ == "__main__":
    main()
