#!/usr/bin/env python3
"""Discover candidate vibe-coded Python repos WITHOUT the throttled commit-search API.

Two stages, both limit-friendly:
  1. DISCOVER — stream a sample of GH Archive hourly dumps (https://data.gharchive.org),
     grep PushEvent payloads for AI-authorship trailers, aggregate repo -> trailer hits.
     GH Archive needs no auth and is not subject to GitHub's search rate limit.
  2. SCORE — for each candidate, use only the GitHub CORE API (5000/hr, NOT search):
       - /repos/{r}/languages          -> Python byte fraction
       - /repos/{r}/commits?per_page=100 -> fraction of recent commits with a trailer
     Rank by recent AI-commit fraction. Provenance only SELECTS; a repo still earns
     'slop' by measuring badly in the benchmark (slop-is-badness-not-provenance).

Writes candidates incrementally to results/slop_candidates.jsonl as each repo is scored,
so progress is visible and a killed run keeps what it found. Stdlib + `gh` + curl/zcat.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from collections import OrderedDict
from pathlib import Path

BENCH = Path(__file__).resolve().parent
OUT = BENCH / "results" / "slop_candidates.jsonl"

TRAILERS = ["Generated with Claude Code", "Co-Authored-By: Claude", "Made with Cursor"]


def gh_json(path: str):
    out = subprocess.run(["gh", "api", path], capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(out.stderr.strip() or f"gh api {path} failed")
    return json.loads(out.stdout)


def discover(hours: list[str]) -> "OrderedDict[str, int]":
    """Stream each GH Archive hour, return repo full_name -> trailer-commit hits seen."""
    grep_args = []
    for t in TRAILERS:
        grep_args += ["-e", t]
    seen: "OrderedDict[str, int]" = OrderedDict()
    for hour in hours:
        url = f"https://data.gharchive.org/{hour}.json.gz"
        print(f"  discover {hour} …", flush=True)
        # curl | zcat | grep (cheap literal filter) | jq (repo names from matches)
        p = subprocess.run(
            f"curl -s {url} | zcat 2>/dev/null | grep -F {' '.join(repr(a) for a in grep_args)} "
            "| jq -r '.repo.name' 2>/dev/null",
            shell=True, capture_output=True, text=True,
        )
        for repo in p.stdout.split():
            seen[repo] = seen.get(repo, 0) + 1
    return seen


def python_fraction(full_name: str) -> float:
    try:
        langs = gh_json(f"/repos/{full_name}/languages")
    except RuntimeError:
        return -1.0
    total = sum(langs.values()) or 1
    return langs.get("Python", 0) / total


def repo_size_kb(full_name: str) -> int:
    try:
        return int(gh_json(f"/repos/{full_name}").get("size", 0))
    except RuntimeError:
        return 0


def whole_history_ai(full_name: str, max_pages: int = 40) -> tuple[int, int, str, bool]:
    """Whole-history AI-commit fraction: paginate ALL commits, count trailers. Core API only.

    Whole-history is the metric we trust (a repo that was vibe-coded throughout, not just
    in its last 100 commits). Returns (ai_commits, commits_scanned, head_sha, truncated).
    `truncated` is True if the repo exceeds max_pages*100 commits (then the fraction is over
    the most-recent max_pages*100). The HEAD SHA is the tip at scan time, recorded so a
    promoted candidate pins to the exact commit we measured."""
    ai = scanned = 0
    head_sha = ""
    truncated = False
    for page in range(1, max_pages + 1):
        try:
            commits = gh_json(f"/repos/{full_name}/commits?per_page=100&page={page}")
        except RuntimeError:
            break
        if not commits:
            break
        if page == 1:
            head_sha = commits[0].get("sha", "")
        scanned += len(commits)
        ai += sum(1 for c in commits
                  if any(t in c.get("commit", {}).get("message", "") for t in TRAILERS))
        if len(commits) < 100:
            break
    else:
        truncated = True
    return ai, scanned, head_sha, truncated


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--hours", nargs="+", required=True,
                    help="GH Archive hour keys, e.g. 2026-06-15-12 2026-06-15-13")
    ap.add_argument("--min-python", type=float, default=0.5)
    ap.add_argument("--min-frac", type=float, default=0.3,
                    help="min fraction of ALL commits carrying a trailer (whole-history)")
    ap.add_argument("--min-commits", type=int, default=30,
                    help="min total commits scanned (maturity floor)")
    ap.add_argument("--min-size-kb", type=int, default=50,
                    help="min repo size in KB (enough code to measure)")
    ap.add_argument("--max-size-kb", type=int, default=300000,
                    help="max repo size in KB (skip monster trees like SAGE; 0 = no cap)")
    ap.add_argument("--max-score", type=int, default=80)
    ap.add_argument("--out", default=str(OUT), help="output JSONL path")
    args = ap.parse_args()

    seen = discover(args.hours)
    print(f"\n{len(seen)} unique candidate repos discovered\n", flush=True)

    ranked = sorted(seen.items(), key=lambda kv: kv[1], reverse=True)[: args.max_score]
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    kept = 0
    with out_path.open("w") as fh:
        for full_name, hits in ranked:
            py = python_fraction(full_name)
            if py < args.min_python:
                print(f"  - {full_name}: python {py:.0%} < {args.min_python:.0%}", flush=True)
                continue
            size = repo_size_kb(full_name)
            if size < args.min_size_kb:
                print(f"  - {full_name}: size {size}KB < {args.min_size_kb}KB", flush=True)
                continue
            if args.max_size_kb and size > args.max_size_kb:
                print(f"  - {full_name}: size {size}KB > {args.max_size_kb}KB (too heavy)", flush=True)
                continue
            ai, total, sha, trunc = whole_history_ai(full_name)
            if total < args.min_commits:
                print(f"  - {full_name}: only {total} commits < {args.min_commits}", flush=True)
                continue
            frac = ai / total if total else 0.0
            keep = frac >= args.min_frac
            print(f"  {'KEEP' if keep else 'drop'} {full_name}: python {py:.0%}, "
                  f"AI {ai}/{total}{'+' if trunc else ''} = {frac:.0%} (seen {hits}x, {size}KB)",
                  flush=True)
            if keep:
                rec = {
                    "name": full_name.split("/")[-1],
                    "url": f"https://github.com/{full_name}",
                    "ref": sha,
                    "python_frac": round(py, 3),
                    "ai_frac": round(frac, 3),
                    "ai_commits": ai, "total_commits": total, "truncated": trunc,
                    "size_kb": size, "archive_hits": hits,
                }
                fh.write(json.dumps(rec) + "\n")
                fh.flush()  # checkpoint: visible immediately, survives a kill
                kept += 1
            time.sleep(0.3)  # gentle on the core API

    print(f"\n=== {kept} candidates written to {OUT} ===")


if __name__ == "__main__":
    main()
