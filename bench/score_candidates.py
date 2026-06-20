#!/usr/bin/env python3
"""Score BigQuery-discovered candidate repos into manifest-ready slop entries.

Input: bench/results/bq_candidates.csv (repo,ai,total,frac) from the githubarchive query.
For each candidate not already in repos.json, fetch via the GitHub CORE API:
  - /repos/{r}        -> size_kb, default_branch, stars, license, fork/archived flags
  - /languages        -> python_frac
  - paginate /commits -> WHOLE-HISTORY ai_frac + total_commits + head SHA (the ref to pin)
Keep Python-dominant, reasonably-sized, low-star (not established) repos that are genuinely
vibe-coded whole-history. Writes manifest-schema JSONL, checkpointing each keeper.

Provenance only SELECTS; each still earns 'slop' by measuring badly. Established/popular
repos that merely use AI assist are excluded by the star ceiling.
"""
from __future__ import annotations

import argparse
import csv
import json
import subprocess
import time
from pathlib import Path

BENCH = Path(__file__).resolve().parent
TRAILERS = ["Generated with Claude Code", "Co-Authored-By: Claude", "Made with Cursor"]


def gh(path):
    out = subprocess.run(["gh", "api", path], capture_output=True, text=True)
    return json.loads(out.stdout) if out.returncode == 0 else None


def whole_history(repo, branch, max_pages=40):
    ai = scanned = 0
    head = ""
    for page in range(1, max_pages + 1):
        commits = gh(f"/repos/{repo}/commits?sha={branch}&per_page=100&page={page}")
        if not commits:
            break
        if page == 1 and commits:
            head = commits[0].get("sha", "")
        scanned += len(commits)
        ai += sum(1 for c in commits
                  if any(t in c.get("commit", {}).get("message", "") for t in TRAILERS))
        if len(commits) < 100:
            break
    return ai, scanned, head


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--limit", type=int, default=150, help="max new candidates to score")
    ap.add_argument("--min-python", type=float, default=0.6)
    ap.add_argument("--min-frac", type=float, default=0.4, help="min whole-history AI fraction")
    ap.add_argument("--min-commits", type=int, default=40)
    ap.add_argument("--min-size-kb", type=int, default=100)
    ap.add_argument("--max-size-kb", type=int, default=300000)
    ap.add_argument("--max-stars", type=int, default=150, help="skip established/popular repos")
    ap.add_argument("--out", default=str(BENCH / "results" / "bq_scored.jsonl"))
    args = ap.parse_args()

    have = set()
    manifest = json.loads((BENCH / "repos.json").read_text())
    for r in manifest["slop"] + manifest["clean"]:
        have.add(r["url"])

    rows = list(csv.DictReader((BENCH / "results" / "bq_candidates.csv").read_text().splitlines()))
    out_path = Path(args.out)
    kept = scored = 0
    with out_path.open("w") as fh:
        for row in rows:
            if scored >= args.limit:
                break
            repo = row["repo"]
            url = f"https://github.com/{repo}"
            if url in have:
                continue
            scored += 1
            info = gh(f"/repos/{repo}")
            if not info:
                print(f"  - {repo}: gone/inaccessible"); continue
            stars = info.get("stargazers_count", 0)
            size = int(info.get("size", 0))
            if info.get("fork") or info.get("archived"):
                print(f"  - {repo}: fork/archived"); continue
            if stars > args.max_stars:
                print(f"  - {repo}: {stars}* > {args.max_stars} (established)"); continue
            if size < args.min_size_kb or size > args.max_size_kb:
                print(f"  - {repo}: size {size}KB out of range"); continue
            langs = gh(f"/repos/{repo}/languages") or {}
            py = langs.get("Python", 0) / (sum(langs.values()) or 1)
            if py < args.min_python:
                print(f"  - {repo}: python {py:.0%}"); continue
            ai, total, head = whole_history(repo, info.get("default_branch", "HEAD"))
            if total < args.min_commits:
                print(f"  - {repo}: {total} commits"); continue
            frac = ai / total if total else 0.0
            lic = (info.get("license") or {}).get("spdx_id", "none")
            if frac < args.min_frac:
                print(f"  drop {repo}: ai {frac:.0%} ({ai}/{total}) {stars}* {lic}"); continue
            rec = {"name": repo.split("/")[-1], "url": url, "ref": head,
                   "python_frac": round(py, 3), "ai_frac": round(frac, 3),
                   "total_commits": total, "size_kb": size}
            fh.write(json.dumps(rec) + "\n"); fh.flush()
            kept += 1
            print(f"  KEEP {repo}: ai {frac:.0%} ({ai}/{total}) py {py:.0%} {stars}* {lic} {size}KB")
            time.sleep(0.2)
    print(f"\n=== scored {scored}, kept {kept} -> {out_path} ===")


if __name__ == "__main__":
    main()
