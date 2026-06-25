#!/usr/bin/env python3
"""Fill exact lifetime contributor counts into contributors.jsonl (separate slow pass).

The main label fetch (fetch_labels.py) is GraphQL-only and fast; contributor count needs a
per-repo REST call (no GraphQL equivalent), bounded by the 5000/hr core budget — a ~2h floor
for ~11k repos regardless of parallelism. So it runs decoupled, on its own rate budget, in
parallel with the GraphQL fetch. label_study.py joins contributors.jsonl when present and
falls back to recent_authors (free, full-coverage proxy) where it isn't.

Resumable: appends, skips done. Stdlib + `gh`.
"""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import fetch_labels as F  # contributors_count, load_targets

STUDY = Path(__file__).resolve().parent
OUT = STUDY / "contributors.jsonl"


def load_done(path: Path) -> set:
    if not path.exists():
        return set()
    return {json.loads(l)["full_name"] for l in path.read_text().splitlines() if l.strip()}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--features", default=str(STUDY / "features.jsonl"))
    ap.add_argument("--out", default=str(OUT))
    args = ap.parse_args()

    targets = F.load_targets(Path(args.features))
    done = load_done(Path(args.out))
    todo = [t for t in targets if t not in done]
    print(f"measured={len(targets)} have={len(done)} todo={len(todo)}", flush=True)

    t0, n = time.time(), 0
    with Path(args.out).open("a") as fh:
        for fn in todo:
            c = F.contributors_count(fn)
            fh.write(json.dumps({"full_name": fn, "contributors": c}) + "\n")
            fh.flush()
            n += 1
            if n % 100 == 0:
                rate = n / max(time.time() - t0, 1e-9)
                print(f"  {n}/{len(todo)}  {rate:.1f}/s eta {(len(todo)-n)/max(rate,1e-9)/60:.0f}m",
                      flush=True)
    print(f"\n=== {n} contributor counts in {(time.time()-t0)/60:.1f}m -> {args.out} ===")


if __name__ == "__main__":
    main()
