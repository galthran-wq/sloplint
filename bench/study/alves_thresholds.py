#!/usr/bin/env python3
"""Derive Alves/SIG metric thresholds and compare how they shift by REFERENCE cohort.

Alves, Ypma & Visser (2010): a threshold for metric M at quantile q is the value k such that q of
the LOC-weighted, per-system-normalized code volume has M <= k. We re-measure a sample with
`--format functions` (per-function value + LOC), then derive 70/80/90 thresholds for different
reference populations to see how much the bar moves:
  - all (random GitHub Python)        - "vs typical"
  - engineered (has_ci ∧ PRs ∧ rel)   - "vs professionally-maintained" (closer to SIG's corpus)
  - non-engineered                    - contrast
  - per LOC bucket                    - size sensitivity

Also reports UNWEIGHTED thresholds to show why Alves weights by LOC. No quality label needed —
the population defines the bands. Streaming clone→functions→reap. Stdlib + git + the release bin.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import threading
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

STUDY = Path(__file__).resolve().parent
BENCH = STUDY.parent
BIN = "/disk1/sloplint/target/release/sloplint"
CFG = str(BENCH / "profiles.toml")
CHECKOUTS = STUDY / "co_alves"
_lock = threading.Lock()
ROWS = []  # (metric_values dict, loc, repo_id, engineered, loc_bucket)

METRICS = ["cyclomatic", "cognitive", "max_nesting", "params"]
LOC_EDGES = [50, 2000, 8000, 30000, 10**12]


def bucket(loc):
    for i in range(len(LOC_EDGES) - 1):
        if LOC_EDGES[i] <= loc < LOC_EDGES[i + 1]:
            return i
    return len(LOC_EDGES) - 2


def measure(repo: dict):
    name = repo["full_name"]
    dest = CHECKOUTS / name.replace("/", "__")
    shutil.rmtree(dest, ignore_errors=True)
    try:
        c = subprocess.run(["git", "clone", "--depth", "1", "--quiet", "--no-tags",
                            "--single-branch", f"https://github.com/{name}", str(dest)],
                           capture_output=True, timeout=180,
                           env={"GIT_TERMINAL_PROMPT": "0", "PATH": "/usr/bin:/bin"})
        if c.returncode != 0:
            return
        out = subprocess.run([BIN, "metrics", str(dest), "--config", CFG, "--scope", "production",
                             "--format", "functions"], capture_output=True, text=True, timeout=180)
        funcs = [json.loads(l) for l in out.stdout.splitlines() if l.strip()]
        total_loc = sum(f.get("loc", 0) for f in funcs)
        if total_loc < 50 or len(funcs) < 3:
            return
        rows = [({m: f.get(m) for m in METRICS}, f.get("loc", 0)) for f in funcs]
        with _lock:
            ROWS.append((rows, total_loc, name, repo["engineered"], bucket(total_loc)))
    except Exception:
        pass
    finally:
        shutil.rmtree(dest, ignore_errors=True)


def alves_threshold(repos, metric, q, weighted=True):
    """q-quantile threshold over LOC-weighted, per-repo-normalized code volume."""
    items = []  # (value, weight)
    for rows, total_loc, *_ in repos:
        for vals, loc in rows:
            v = vals.get(metric)
            if v is None:
                continue
            w = (loc / total_loc) if (weighted and total_loc) else 1.0
            items.append((v, w))
    if not items:
        return None
    items.sort(key=lambda t: t[0])
    tot = sum(w for _, w in items)
    cum = 0.0
    for v, w in items:
        cum += w
        if cum / tot >= q:
            return v
    return items[-1][0]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=600)
    ap.add_argument("--workers", type=int, default=12)
    args = ap.parse_args()

    eng = set()
    for l in open(STUDY / "labels.jsonl"):
        r = json.loads(l)
        if r.get("ok") and r.get("has_ci") and (r.get("merged_prs") or 0) > 0 and (r.get("releases") or 0) > 0:
            eng.add(r["full_name"])
    repos = []
    for l in open(STUDY / "features.jsonl"):
        r = json.loads(l)
        if r.get("ok") and (r.get("m.total_loc") or 0) >= 50:
            repos.append({"full_name": r["full_name"], "engineered": r["full_name"] in eng})
    # interleave so the sample has both classes
    repos = repos[:: max(1, len(repos) // args.n)][: args.n]
    CHECKOUTS.mkdir(parents=True, exist_ok=True)
    print(f"measuring {len(repos)} repos (functions feed)…", flush=True)
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futs = [pool.submit(measure, r) for r in repos]
        for i, _ in enumerate(as_completed(futs), 1):
            if i % 50 == 0:
                print(f"  {i}/{len(repos)}  collected={len(ROWS)}", flush=True)
    shutil.rmtree(CHECKOUTS, ignore_errors=True)

    allr = ROWS
    engr = [x for x in ROWS if x[3]]
    nonr = [x for x in ROWS if not x[3]]
    print(f"\ncollected: all={len(allr)} engineered={len(engr)} non={len(nonr)}\n")

    out = ["# Alves/SIG thresholds by reference cohort\n",
           f"Repos: all={len(allr)}, engineered={len(engr)}, non-engineered={len(nonr)}. "
           "Threshold = metric value at the q-quantile of LOC-weighted code volume.\n"]
    for metric in ["cyclomatic", "cognitive"]:
        out.append(f"\n## {metric} — LOC-weighted thresholds (70 / 80 / 90 %)\n")
        out.append("| reference | 70% | 80% | 90% |")
        out.append("|---|---:|---:|---:|")
        for label, cohort in [("all (typical GitHub)", allr), ("engineered", engr), ("non-engineered", nonr)]:
            t = [alves_threshold(cohort, metric, q) for q in (.70, .80, .90)]
            out.append(f"| {label} | {t[0]} | {t[1]} | {t[2]} |")
        # per LOC bucket (all)
        out.append(f"\n### {metric} by repo size (all repos)\n")
        out.append("| LOC bucket | n | 70% | 80% | 90% |")
        out.append("|---|---:|---:|---:|---:|")
        for b in range(len(LOC_EDGES) - 1):
            cb = [x for x in allr if x[4] == b]
            if len(cb) < 10:
                continue
            t = [alves_threshold(cb, metric, q) for q in (.70, .80, .90)]
            lo, hi = LOC_EDGES[b], LOC_EDGES[b + 1]
            out.append(f"| {lo}–{hi if hi<10**11 else '∞'} | {len(cb)} | {t[0]} | {t[1]} | {t[2]} |")
        # weighted vs unweighted (all)
        out.append(f"\n### {metric} weighted vs unweighted (all) — why Alves weights by LOC\n")
        out.append("| | 70% | 80% | 90% |")
        out.append("|---|---:|---:|---:|")
        tw = [alves_threshold(allr, metric, q, True) for q in (.70, .80, .90)]
        tu = [alves_threshold(allr, metric, q, False) for q in (.70, .80, .90)]
        out.append(f"| LOC-weighted | {tw[0]} | {tw[1]} | {tw[2]} |")
        out.append(f"| unweighted | {tu[0]} | {tu[1]} | {tu[2]} |")
    Path(STUDY / "alves_thresholds_report.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
