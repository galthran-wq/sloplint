#!/usr/bin/env python3
"""Fast scorer for a large BigQuery candidate CSV (repo,ai,total,frac).

Trusts the BigQuery 9-month AI-push fraction as `ai_frac` (skips per-repo whole-history
pagination — the slow part), so each candidate costs only 3 core-API calls:
  /repos/{r}            -> size_kb, stars, default_branch, fork/archived, license
  /languages            -> python_frac
  /commits/{branch}?1   -> head SHA (the ref to pin)
Keeps Python-dominant, sized, low-star, non-fork/active repos. Handles GitHub rate limits
by sleeping until the core quota resets. Checkpoints each keeper to JSONL.

NOTE: ai_frac here is the 9-month windowed fraction (githubarchive 2025-10..2026-06), not the
full-history value used for the earlier hand-vetted slop entries. Documented, intentional —
these repos are mostly new, so the window ~= whole history.
"""
from __future__ import annotations

import argparse
import csv
import json
import subprocess
import time
from pathlib import Path

BENCH = Path(__file__).resolve().parent


def gh(path):
    """gh api with rate-limit backoff: on 403/limit, sleep until core resets, then retry."""
    for _ in range(8):
        out = subprocess.run(["gh", "api", path], capture_output=True, text=True)
        if out.returncode == 0:
            try:
                return json.loads(out.stdout)
            except json.JSONDecodeError:
                return None
        err = out.stderr.lower()
        if "rate limit" in err or "secondary" in err or "was submitted too quickly" in err:
            rl = subprocess.run(["gh", "api", "/rate_limit"], capture_output=True, text=True)
            wait = 60
            try:
                reset = json.loads(rl.stdout)["resources"]["core"]["reset"]
                wait = max(10, min(960, int(reset - time.time()) + 5))
            except Exception:
                pass
            print(f"    rate limit; sleeping {wait}s", flush=True)
            time.sleep(wait)
            continue
        return None  # 404 / gone / other — give up on this repo
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--csv", default=str(BENCH / "results" / "bq_candidates_loose.csv"))
    ap.add_argument("--limit", type=int, default=2500)
    ap.add_argument("--min-python", type=float, default=0.6)
    ap.add_argument("--min-frac", type=float, default=0.4)
    ap.add_argument("--min-commits", type=int, default=10)
    ap.add_argument("--min-size-kb", type=int, default=100)
    ap.add_argument("--max-size-kb", type=int, default=300000)
    ap.add_argument("--max-stars", type=int, default=150)
    ap.add_argument("--out", default=str(BENCH / "results" / "bq_fast_scored.jsonl"))
    args = ap.parse_args()

    manifest = json.loads((BENCH / "repos.json").read_text())
    have = {r["url"] for r in manifest["slop"] + manifest["clean"]}

    rows = list(csv.DictReader(Path(args.csv).read_text().splitlines()))
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
            frac = float(row["frac"]); total = int(row["total"])
            if frac < args.min_frac or total < args.min_commits:
                continue
            scored += 1
            info = gh(f"/repos/{repo}")
            if not info:
                continue  # gone/private
            if info.get("fork") or info.get("archived"):
                continue
            stars = info.get("stargazers_count", 0)
            size = int(info.get("size", 0))
            if stars > args.max_stars or size < args.min_size_kb or size > args.max_size_kb:
                continue
            langs = gh(f"/repos/{repo}/languages") or {}
            py = langs.get("Python", 0) / (sum(langs.values()) or 1)
            if py < args.min_python:
                continue
            branch = info.get("default_branch", "HEAD")
            commits = gh(f"/repos/{repo}/commits?sha={branch}&per_page=1")
            sha = commits[0]["sha"] if commits else ""
            if not sha:
                continue
            rec = {"name": repo.split("/")[-1], "url": url, "ref": sha,
                   "python_frac": round(py, 3), "ai_frac": frac,
                   "total_commits": total, "size_kb": size}
            fh.write(json.dumps(rec) + "\n"); fh.flush()
            kept += 1
            if kept % 10 == 0:
                print(f"  kept {kept} / scored {scored}", flush=True)
    print(f"\n=== scored {scored}, kept {kept} -> {out_path} ===")


if __name__ == "__main__":
    main()
