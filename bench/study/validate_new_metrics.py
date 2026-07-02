#!/usr/bin/env python3
"""Do the new #265 CK metrics (RFC, TCC, LCC, LCOM*, fan-in/out, CBO-mod, NOSI, *Qty counters)
discriminate quality? — the test we couldn't run before they existed.

Re-measures a size-controlled sample (mid LOC bucket) with the #265 binary, capturing the new
per-class and per-function fields, then compares each metric's per-repo mean across collaboration
labels (contributors / has_ci / engineered) + stars (negative control). For each metric we know
its polarity, so ✅ = the 'good/mature' side is better, ⚠ = worse, ≈ flat. Class metrics need ≥5
classes. Stdlib + git + the release bin.
"""

from __future__ import annotations

import json
import shutil
import statistics as st
import subprocess
import threading
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

STUDY = Path(__file__).resolve().parent
BENCH = STUDY.parent
BIN = "/disk1/sloplint/target/release/sloplint"
CFG = str(BENCH / "profiles.toml")
CO = STUDY / "co_new"
_lock = threading.Lock()
ROWS = {}  # full_name -> {metric: mean_value, "_nclasses": n}

# (field, polarity)  hi = higher-is-worse, lo = higher-is-better
CLASS_M = [("rfc", "hi"), ("cbo_modified", "hi"), ("fan_in", "hi"), ("fan_out", "hi"),
           ("nosi", "hi"), ("tcc", "lo"), ("lcc", "lo"), ("lcom_star", "hi")]
FUNC_M = [("loop_qty", "hi"), ("comparisons_qty", "hi"), ("variables_qty", "hi"),
          ("unique_words_qty", "hi"), ("math_ops_qty", "hi")]


def measure(fn):
    dest = CO / fn.replace("/", "__")
    shutil.rmtree(dest, ignore_errors=True)
    try:
        c = subprocess.run(["git", "clone", "--depth", "1", "--quiet", "--no-tags", "--single-branch",
                            f"https://github.com/{fn}", str(dest)], capture_output=True, timeout=180,
                           env={"GIT_TERMINAL_PROMPT": "0", "PATH": "/usr/bin:/bin"})
        if c.returncode != 0:
            return
        cls = [json.loads(l) for l in subprocess.run(
            [BIN, "metrics", str(dest), "--config", CFG, "--scope", "production", "--format", "classes"],
            capture_output=True, text=True, timeout=180).stdout.splitlines() if l.strip()]
        fns = [json.loads(l) for l in subprocess.run(
            [BIN, "metrics", str(dest), "--config", CFG, "--scope", "production", "--format", "functions"],
            capture_output=True, text=True, timeout=180).stdout.splitlines() if l.strip()]
        rec = {"_nclasses": len(cls)}
        for m, _ in CLASS_M:
            vs = [c[m] for c in cls if c.get(m) is not None]
            if vs:
                rec[m] = st.mean(vs)
        for m, _ in FUNC_M:
            vs = [f[m] for f in fns if f.get(m) is not None]
            if vs:
                rec[m] = st.mean(vs)
        with _lock:
            ROWS[fn] = rec
    except Exception:
        pass
    finally:
        shutil.rmtree(dest, ignore_errors=True)


def main():
    import random
    # mid-bucket repos (8k-30k LOC) with labels
    eng = {}
    for l in open(STUDY / "labels.jsonl"):
        r = json.loads(l)
        if r.get("ok"):
            eng[r["full_name"]] = r
    meta = {}
    for l in open(STUDY / "features.jsonl"):
        r = json.loads(l)
        if r.get("ok") and 8000 <= (r.get("m.total_loc") or 0) < 30000 and r["full_name"] in eng:
            lb = eng[r["full_name"]]
            meta[r["full_name"]] = {
                "stars": r.get("stars"), "contributors": None,  # fill below
                "has_ci": 1 if lb.get("has_ci") else 0,
                "engineered": 1 if (lb.get("has_ci") and (lb.get("merged_prs") or 0) > 0 and (lb.get("releases") or 0) > 0) else 0}
    for l in open(STUDY / "contributors.jsonl"):
        r = json.loads(l)
        if r["full_name"] in meta:
            meta[r["full_name"]]["contributors"] = r.get("contributors")
    targets = list(meta)
    random.Random(0).shuffle(targets)
    targets = targets[:1600]
    CO.mkdir(parents=True, exist_ok=True)
    print(f"measuring {len(targets)} mid-bucket repos with #265 binary…", flush=True)
    with ThreadPoolExecutor(max_workers=14) as pool:
        futs = [pool.submit(measure, fn) for fn in targets]
        for i, _ in enumerate(as_completed(futs), 1):
            if i % 200 == 0:
                print(f"  {i}/{len(targets)} (collected {len(ROWS)})", flush=True)
    shutil.rmtree(CO, ignore_errors=True)

    LABELS = ["contributors", "has_ci", "engineered", "stars"]

    def verdict(metric, pol, label, need_classes):
        repos = [fn for fn in ROWS if metric in ROWS[fn] and meta.get(fn, {}).get(label) is not None
                 and (not need_classes or ROWS[fn]["_nclasses"] >= 5)]
        if label in ("has_ci", "engineered"):
            hi = [fn for fn in repos if meta[fn][label] == 1]; lo = [fn for fn in repos if meta[fn][label] == 0]
        else:
            vs = sorted(meta[fn][label] for fn in repos if meta[fn][label] is not None)
            if len(vs) < 30:
                return "·"
            a, b = vs[len(vs)//3], vs[2*len(vs)//3]
            hi = [fn for fn in repos if (meta[fn][label] or -1) >= b]
            lo = [fn for fn in repos if (meta[fn][label] or 1e9) <= a]
        h = [ROWS[fn][metric] for fn in hi]; n = [ROWS[fn][metric] for fn in lo]
        if len(h) < 20 or len(n) < 20:
            return "·"
        mh, mn = st.median(h), st.median(n)
        better = (mh < mn) if pol == "hi" else (mh > mn)
        flat = abs(mh - mn) < 0.05 * max(abs(mh), abs(mn), 1e-9)
        mark = "≈" if flat else ("✅" if better else "⚠")
        return f"{mh:.2f}/{mn:.2f}{mark}"

    out = [f"# New #265 metrics — do they discriminate quality? (size-controlled 8–30k LOC, N={len(ROWS)})\n",
           "Per-repo mean of the metric; hi = top tercile of label. ✅ good/mature side better · ⚠ worse · ≈ flat.\n",
           "| metric (polarity) | " + " | ".join(LABELS) + " |",
           "|" + "---|" * (len(LABELS) + 1)]
    for m, pol in CLASS_M:
        out.append(f"| {m} ({pol}, class) | " + " | ".join(verdict(m, pol, L, True) for L in LABELS) + " |")
    for m, pol in FUNC_M:
        out.append(f"| {m} ({pol}, func) | " + " | ".join(verdict(m, pol, L, False) for L in LABELS) + " |")
    Path(STUDY / "validate_new_metrics_report.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
