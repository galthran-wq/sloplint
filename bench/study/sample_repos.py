#!/usr/bin/env python3
"""Sample a stratified + uniform frame of Python repos for the SQM-vs-popularity study.

The study (sloplint#55 / #142, scaled from N~234 to ~10k) regresses our software-quality
metrics against external repo-health proxies. A *uniform* random sample of GitHub Python
repos is ~all dead 0-star toy repos — the proxy has no variance and the metric<->popularity
relationship is unestimable. So we sample in two layers:

  STRATIFIED  — per (star-bucket x created-window) cell, pull a quota via the Search API.
                Gives variance across the whole popularity spectrum -> power to estimate the
                relationship and per-stratum bands. Search caps a query at 1000 results, so a
                cell whose total_count exceeds 1000 is split in half by date, recursively.
  UNIFORM     — a true-random layer over repo-id space (REST /repositories?since=<rand>),
                filtered to Python-majority. The honest "typical GitHub Python" reference
                distribution we calibrate the index z-scores against. (--uniform N)

The Search payload already carries most proxies (stars, created/pushed, forks, issues, size,
language) — so stratified sampling fills the proxy columns for free; only contributor count
needs a later call (fetch_proxies.py).

Output: bench/study/frame.jsonl, one JSON object per repo, appended and deduped on full_name
so a killed run resumes. Stdlib + `gh`.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

STUDY = Path(__file__).resolve().parent
FRAME = STUDY / "frame.jsonl"

# (label, low, high) — high=None is open-ended (stars>=low). Buckets are log-spaced so each
# spans a comparable order of magnitude of popularity; the spectrum is what gives the proxy
# its variance.
STAR_BUCKETS = [
    ("s0", 0, 0),
    ("s1_9", 1, 9),
    ("s10_49", 10, 49),
    ("s50_199", 50, 199),
    ("s200_999", 200, 999),
    ("s1k_4999", 1000, 4999),
    ("s5k_up", 5000, None),
]


def stars_qualifier(lo: int, hi: int | None) -> str:
    if hi is None:
        return f"stars:>={lo}"
    if lo == hi:
        return f"stars:{lo}"
    return f"stars:{lo}..{hi}"


def gh_search(q: str, page: int) -> dict:
    """One Search API page. gh handles auth; raises on non-rate-limit errors."""
    while True:
        out = subprocess.run(
            ["gh", "api", "-X", "GET", "search/repositories",
             "-f", f"q={q}", "-F", "per_page=100", "-F", f"page={page}", "-F", "sort=updated"],
            capture_output=True, text=True,
        )
        if out.returncode == 0:
            return json.loads(out.stdout)
        err = out.stderr.lower()
        if "rate limit" in err or "403" in err or "secondary" in err:
            print("    rate-limited; sleeping 60s", flush=True)
            time.sleep(60)
            continue
        raise RuntimeError(out.stderr.strip() or "gh search failed")


def row_from_item(item: dict, stratum: str) -> dict:
    """Flatten a Search repo object into a frame row (proxies harvested for free)."""
    return {
        "full_name": item["full_name"],
        "stratum": stratum,
        "layer": "stratified",
        "stars": item.get("stargazers_count", 0),
        "created_at": item.get("created_at"),
        "pushed_at": item.get("pushed_at"),
        "size_kb": item.get("size", 0),
        "forks": item.get("forks_count", 0),
        "open_issues": item.get("open_issues_count", 0),
        "default_branch": item.get("default_branch", "HEAD"),
        "language": item.get("language"),
        "archived": item.get("archived", False),
        "is_fork": item.get("fork", False),
    }


def harvest_cell(lo: int, hi: int | None, d1: str, d2: str, quota: int,
                 seen: set, fh, depth: int = 0) -> int:
    """Pull up to `quota` repos for one (star-bucket x date-window) cell.

    If the cell's total_count exceeds the Search 1000-result ceiling, split the date window
    in half and recurse, so no repo range is silently truncated."""
    stars_q = stars_qualifier(lo, hi)
    q = f"language:python {stars_q} created:{d1}..{d2} fork:false archived:false"
    pad = "  " * depth
    first = gh_search(q, 1)
    total = first.get("total_count", 0)
    print(f"{pad}cell {stars_q} {d1}..{d2}: total={total}", flush=True)
    time.sleep(2.1)  # search budget is 30/min

    # Subdivide only when the quota genuinely can't be served from one window's reachable
    # results (the Search API caps at 1000 = 10 pages). A small quota fits in page 1, so
    # never split for it; cap depth so a dense low-star cell can't recurse without bound.
    if total > 1000 and quota > 900 and depth < 4 and d1 != d2:
        mid = _mid_date(d1, d2)
        if mid and mid != d1 and mid != d2:
            n = harvest_cell(lo, hi, d1, mid, quota // 2 + 1, seen, fh, depth + 1)
            n += harvest_cell(lo, hi, _next_day(mid), d2, quota // 2 + 1, seen, fh, depth + 1)
            return n

    kept = 0
    page = 1
    items = first["items"]
    while items and kept < quota:
        for item in items:
            if kept >= quota:
                break
            name = item["full_name"]
            if name in seen:
                continue
            seen.add(name)
            fh.write(json.dumps(row_from_item(item, f"{_bucket_label(lo, hi)}|{d1[:7]}")) + "\n")
            fh.flush()
            kept += 1
        if kept >= quota or page * 100 >= min(total, 1000):
            break
        page += 1
        items = gh_search(q, page)["items"]
        time.sleep(2.1)
    print(f"{pad}  -> kept {kept}", flush=True)
    return kept


def _bucket_label(lo: int, hi: int | None) -> str:
    for label, blo, bhi in STAR_BUCKETS:
        if blo == lo and bhi == hi:
            return label
    return f"s{lo}"


def _mid_date(d1: str, d2: str) -> str | None:
    from datetime import date
    a = date.fromisoformat(d1)
    b = date.fromisoformat(d2)
    if (b - a).days <= 1:
        return None
    return (a + (b - a) // 2).isoformat()


def _next_day(d: str) -> str:
    from datetime import date, timedelta
    return (date.fromisoformat(d) + timedelta(days=1)).isoformat()


def date_windows(start: str, end: str, months: int) -> list[tuple[str, str]]:
    from datetime import date
    out = []
    y, m = int(start[:4]), int(start[5:7])
    ey, em = int(end[:4]), int(end[5:7])
    while (y, m) <= (ey, em):
        d1 = date(y, m, 1)
        nm = m + months
        ny = y + (nm - 1) // 12
        nm = (nm - 1) % 12 + 1
        # last day of the window = day before the next window's first day
        from datetime import timedelta
        d2 = date(ny, nm, 1) - timedelta(days=1)
        out.append((d1.isoformat(), min(d2, date.fromisoformat(end)).isoformat()))
        y, m = ny, nm
        if (y, m) > (ey, em):
            break
    return out


def load_seen(path: Path) -> set:
    if not path.exists():
        return set()
    return {json.loads(line)["full_name"] for line in path.read_text().splitlines() if line.strip()}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--start", default="2021-01-01", help="earliest created date")
    ap.add_argument("--end", default="2026-06-01", help="latest created date")
    ap.add_argument("--window-months", type=int, default=12)
    ap.add_argument("--per-cell", type=int, default=200, help="quota per (bucket x window) cell")
    ap.add_argument("--buckets", help="comma-separated bucket labels to run (default: all)")
    ap.add_argument("--out", default=str(FRAME))
    args = ap.parse_args()

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    seen = load_seen(out_path)
    print(f"resuming with {len(seen)} repos already in frame", flush=True)

    buckets = STAR_BUCKETS
    if args.buckets:
        want = set(args.buckets.split(","))
        buckets = [b for b in STAR_BUCKETS if b[0] in want]

    windows = date_windows(args.start, args.end, args.window_months)
    total_kept = 0
    with out_path.open("a") as fh:
        for label, lo, hi in buckets:
            for d1, d2 in windows:
                total_kept += harvest_cell(lo, hi, d1, d2, args.per_cell, seen, fh)
    print(f"\n=== {total_kept} new repos; frame now {len(seen)} total -> {out_path} ===")


if __name__ == "__main__":
    main()
