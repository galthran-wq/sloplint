#!/usr/bin/env python3
"""Fetch external labels/covariates for the sampled frame via GitHub GraphQL + REST.

The popularity study showed stars/forks ⊥ our metrics once size is controlled. This pulls a
broad set of *process/health/provenance* signals — conceptually closer to "is this engineered
or vibe-dumped", plus the project's central covariate: how much of the repo was AI-authored.

Per repo (mostly from one 100-commit history window — cheap):
  PROCESS / DISCIPLINE
    has_ci           .github/workflows tree exists
    merged_prs       merged-PR count (PR workflow vs solo direct-push)
    reviewed_rate    fraction of a recent merged-PR sample with >=1 review (noisy)
    releases         release count (maturity)
  DEFECT
    bugfix_ratio     fix/bug/revert fraction of the last-100 commit subjects (MSR defect proxy)
  TEAM
    contributors     lifetime contributor count (REST Link-header; capped ~500 by GitHub)
    recent_authors   distinct commit authors in the last 100 (active team size)
  ACTIVITY / CADENCE  (from committedDate of the last 100 — "constant commits vs once a week")
    commits_total    total commits on default branch
    commits_per_week recent velocity over the last-100 window
    median_gap_days  median inter-commit gap
    active_week_frac distinct ISO-weeks with a commit / weeks spanned (1.0 = every week)
    recent_span_days calendar span of the last-100 window
  PROVENANCE  (slop-is-badness-NOT-provenance: a covariate, never a quality verdict)
    ai_share         fraction of last-100 commits carrying any AI-tool trailer
    claude_share     fraction carrying a Claude/Claude-Code trailer specifically

We clone shallow + reap, so git history is gone; GraphQL/REST recover these without re-cloning.
GraphQL ~1 pt/repo and REST ~1 call/repo run on SEPARATE hourly budgets, so the whole measured
frame (~11k) fits in ~2-3 rate-limit hours. Resumable: appends to labels.jsonl, skips done.
"""

from __future__ import annotations

import argparse
import json
import re
import statistics
import subprocess
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime
from pathlib import Path

_lock = threading.Lock()

STUDY = Path(__file__).resolve().parent
FEATURES = STUDY / "features.jsonl"
LABELS = STUDY / "labels.jsonl"

FIX_RE = re.compile(r"\b(fix|fixes|fixed|bug|bugfix|hotfix|patch|revert|regression)\b", re.I)
# AI-tool commit trailers/markers. Claude-specific is a subset (claude_share); the rest broaden
# to ai_share. Provenance only — measured, never scored as "bad".
CLAUDE_RE = re.compile(r"co-authored-by:\s*claude|generated with claude code|claude-session|"
                       r"noreply@anthropic\.com|claude\.ai/code", re.I)
AI_RE = re.compile(r"co-authored-by:\s*claude|generated with claude code|claude-session|"
                   r"noreply@anthropic\.com|claude\.ai/code|"
                   r"made with cursor|co-authored-by:\s*cursor|"
                   r"co-authored-by:\s*copilot|github copilot|"
                   r"bolt\.new|lovable\.dev|generated with \[?devin|"
                   r"windsurf|codeium|aider", re.I)

REPO_BLOCK = """
r{i}: repository(owner:{owner}, name:{name}) {{
  releases {{ totalCount }}
  mergedPRs: pullRequests(states:MERGED) {{ totalCount }}
  reviewSample: pullRequests(states:MERGED, first:40, orderBy:{{field:UPDATED_AT, direction:DESC}}) {{
    nodes {{ reviews {{ totalCount }} }}
  }}
  ci: object(expression:"HEAD:.github/workflows") {{ ... on Tree {{ entries {{ name }} }} }}
  defaultBranchRef {{ target {{ ... on Commit {{
    history(first:100) {{ totalCount nodes {{ message committedDate author {{ name user {{ login }} }} }} }}
  }} }} }}
}}"""


def gql(query: str) -> dict:
    while True:
        out = subprocess.run(["gh", "api", "graphql", "-f", f"query={query}"],
                             capture_output=True, text=True)
        if out.returncode == 0:
            return json.loads(out.stdout)
        err = out.stderr.lower()
        # Genuine rate/abuse limit: back off. 502/504/timeout = query too heavy or transient —
        # raise fast so the caller falls back to light single-repo queries instead of stalling.
        if "secondary rate limit" in err or "exceeded a rate limit" in err or "abuse" in err:
            print("    rate-limited; sleeping 60s", flush=True)
            time.sleep(60)
            continue
        raise RuntimeError(out.stderr.strip()[:200] or "graphql failed")


def contributors_count(full_name: str) -> int | None:
    """Lifetime contributor count via the REST Link-header trick (1 anon entry/page; the
    rel=last page number == the count). GitHub caps this at ~500 for large repos."""
    out = subprocess.run(
        ["gh", "api", "-i", f"repos/{full_name}/contributors?per_page=1&anon=true"],
        capture_output=True, text=True)
    if out.returncode != 0:
        return None
    head, _, body = out.stdout.partition("\r\n\r\n")
    m = re.search(r'[?&]page=(\d+)>;\s*rel="last"', head)
    if m:
        return int(m.group(1))
    # no Link header => 0 or 1 contributors; count the body array
    try:
        arr = json.loads(body)
        return len(arr) if isinstance(arr, list) else None
    except json.JSONDecodeError:
        return None


def _dt(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def cadence(dates: list[str]) -> dict:
    """Activity/cadence from the last-100 committedDate list (newest-first)."""
    if len(dates) < 2:
        return {"commits_per_week": None, "median_gap_days": None,
                "active_week_frac": None, "recent_span_days": None}
    ds = sorted(_dt(d) for d in dates)
    span_days = (ds[-1] - ds[0]).total_seconds() / 86400
    gaps = [(ds[i + 1] - ds[i]).total_seconds() / 86400 for i in range(len(ds) - 1)]
    weeks = {d.isocalendar()[:2] for d in ds}
    span_weeks = max(span_days / 7, 1e-9)
    return {
        "commits_per_week": round(len(ds) / span_weeks, 3) if span_days > 0 else None,
        "median_gap_days": round(statistics.median(gaps), 3),
        "active_week_frac": round(len(weeks) / (span_weeks + 1), 3),
        "recent_span_days": round(span_days, 1),
    }


def parse_repo(node: dict | None, full_name: str) -> dict:
    if not node:
        return {"full_name": full_name, "ok": False}
    hist = (node.get("defaultBranchRef") or {}).get("target") or {}
    history = hist.get("history") or {}
    nodes = history.get("nodes", [])
    msgs = [n["message"] for n in nodes if n.get("message")]
    subjects = [m.splitlines()[0] for m in msgs]
    fix = sum(1 for s in subjects if FIX_RE.search(s))
    ai = sum(1 for m in msgs if AI_RE.search(m))
    claude = sum(1 for m in msgs if CLAUDE_RE.search(m))
    authors = {(n.get("author") or {}).get("name") or
               ((n.get("author") or {}).get("user") or {}).get("login")
               for n in nodes if n.get("author")}
    authors.discard(None)
    dates = [n["committedDate"] for n in nodes if n.get("committedDate")]
    sample = (node.get("reviewSample") or {}).get("nodes", [])
    reviewed = sum(1 for p in sample if (p.get("reviews") or {}).get("totalCount", 0) > 0)
    ci = node.get("ci") or {}
    rec = {
        "full_name": full_name, "ok": True,
        "commits_total": history.get("totalCount", 0),
        "bugfix_ratio": round(fix / len(msgs), 4) if msgs else None,
        "ai_share": round(ai / len(msgs), 4) if msgs else None,
        "claude_share": round(claude / len(msgs), 4) if msgs else None,
        "recent_authors": len(authors),
        "has_ci": bool(ci.get("entries")),
        "releases": (node.get("releases") or {}).get("totalCount", 0),
        "merged_prs": (node.get("mergedPRs") or {}).get("totalCount", 0),
        "reviewed_rate": round(reviewed / len(sample), 4) if sample else None,
    }
    rec.update(cadence(dates))
    return rec


def build_query(batch: list[str]) -> tuple[str, dict]:
    blocks, alias = [], {}
    for i, fn in enumerate(batch):
        owner, name = fn.split("/", 1)
        alias[f"r{i}"] = fn
        blocks.append(REPO_BLOCK.format(i=i, owner=json.dumps(owner), name=json.dumps(name)))
    q = "query{ rateLimit{ cost remaining } " + " ".join(blocks) + " }"
    return q, alias


def load_targets(features: Path) -> list[str]:
    return [json.loads(l)["full_name"] for l in features.read_text().splitlines()
            if l.strip() and json.loads(l).get("ok")]


def load_done(path: Path) -> set:
    if not path.exists():
        return set()
    return {json.loads(l)["full_name"] for l in path.read_text().splitlines() if l.strip()}


def process_batch(batch: list[str]) -> list[dict]:
    """One aliased GraphQL batch → parsed records. Falls back to single-repo on batch error
    so one bad repo (504, deleted) doesn't drop the other 19. GraphQL-only (no contributors —
    that's the rate-limited fetch_contributors.py pass)."""
    q, alias = build_query(batch)
    try:
        data = gql(q).get("data", {})
        return [parse_repo(data.get(al), fn) for al, fn in alias.items()]
    except RuntimeError:
        recs = []
        for fn in batch:
            try:
                d1, _ = build_query([fn])
                recs.append(parse_repo(gql(d1)["data"].get("r0"), fn))
            except Exception:
                recs.append({"full_name": fn, "ok": False})
        return recs


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--features", default=str(FEATURES))
    ap.add_argument("--out", default=str(LABELS))
    ap.add_argument("--batch", type=int, default=15)
    ap.add_argument("--workers", type=int, default=10)
    ap.add_argument("--limit", type=int, default=0)
    args = ap.parse_args()

    targets = load_targets(Path(args.features))
    done = load_done(Path(args.out))
    todo = [t for t in targets if t not in done]
    if args.limit:
        todo = todo[: args.limit]
    batches = [todo[i:i + args.batch] for i in range(0, len(todo), args.batch)]
    print(f"measured={len(targets)} labeled={len(done)} todo={len(todo)} "
          f"batches={len(batches)} workers={args.workers}", flush=True)

    t0 = time.time()
    n = 0
    with Path(args.out).open("a") as fh, ThreadPoolExecutor(max_workers=args.workers) as pool:
        futs = [pool.submit(process_batch, b) for b in batches]
        for i, fut in enumerate(as_completed(futs), 1):
            recs = fut.result()
            with _lock:
                for rec in recs:
                    fh.write(json.dumps(rec) + "\n")
                    n += 1
                fh.flush()
            if i % 20 == 0 or i == len(batches):
                rate = n / max(time.time() - t0, 1e-9)
                print(f"  {n}/{len(todo)}  {rate:.0f}/s eta {(len(todo)-n)/max(rate,1e-9)/60:.0f}m",
                      flush=True)
    print(f"\n=== {n} labeled in {(time.time()-t0)/60:.1f}m -> {args.out} ===")


if __name__ == "__main__":
    main()
