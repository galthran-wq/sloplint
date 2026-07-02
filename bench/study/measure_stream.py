#!/usr/bin/env python3
"""Stream a repo frame through sloplint: clone -> measure -> append -> reap.

Scaled successor to run.py. run.py keeps every checkout on disk (fine for ~300 cohort repos,
impossible for ~10k). This clones each repo shallow, runs `sloplint metrics` (production scope),
flattens the whole panel into one record, appends it to features.jsonl, then DELETES the
checkout — so disk stays flat regardless of frame size.

Robust + resumable:
  - a worker pool clones/measures concurrently (network-bound),
  - every full_name already in the output (ok OR permanently failed) is skipped,
  - per-repo timeout + size cap; a failed repo is recorded `ok:false` (not retried unless
    --retry-failed) so one bad repo never blocks the run,
  - output is append-only JSONL (schema-flexible: the nested panel flattens to dotted keys,
    so panel growth doesn't break old rows). study.py joins this with proxies.csv.

Stdlib + `git` + the sloplint release binary. JSONL not CSV: the panel is nested and grows;
flat per-repo records keyed by dotted paths are robust to that.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

STUDY = Path(__file__).resolve().parent
BENCH = STUDY.parent
FRAME = STUDY / "frame.jsonl"
FEATURES = STUDY / "features.jsonl"
CHECKOUTS = STUDY / "checkouts"
DEFAULT_BIN = "/disk1/sloplint/target/release/sloplint"
DEFAULT_CFG = str(BENCH / "profiles.toml")

_write_lock = threading.Lock()


def flatten(prefix: str, obj, out: dict) -> None:
    """Recurse a nested dict, emitting scalar leaves as dotted keys. Lists are dropped
    (the only list is packages.cycles.members — structural, not a feature)."""
    for k, v in obj.items():
        key = f"{prefix}{k}"
        if isinstance(v, dict):
            flatten(key + ".", v, out)
        elif isinstance(v, (int, float, bool)) or v is None:
            out[key] = v
        # lists / strings skipped


def measure_one(row: dict, binary: str, cfg: str, timeout: int, max_kb: int) -> dict:
    name = row["full_name"]
    base = {k: row.get(k) for k in (
        "full_name", "stratum", "layer", "stars", "created_at", "pushed_at",
        "size_kb", "forks", "open_issues", "language", "archived", "is_fork")}

    if max_kb and row.get("size_kb", 0) > max_kb:
        return {**base, "ok": False, "error": f"size {row['size_kb']}KB > {max_kb}KB cap"}

    dest = CHECKOUTS / name.replace("/", "__")
    shutil.rmtree(dest, ignore_errors=True)
    dest.parent.mkdir(parents=True, exist_ok=True)
    url = f"https://github.com/{name}"
    try:
        clone = subprocess.run(
            ["git", "-c", "core.askPass=true", "clone", "--depth", "1", "--quiet",
             "--no-tags", "--single-branch", url, str(dest)],
            capture_output=True, text=True, timeout=timeout,
            env={"GIT_TERMINAL_PROMPT": "0", "GIT_LFS_SKIP_SMUDGE": "1", "PATH": _PATH},
        )
        if clone.returncode != 0:
            return {**base, "ok": False, "error": "clone: " + (clone.stderr.strip()[:200] or "failed")}

        sha = subprocess.run(["git", "-C", str(dest), "rev-parse", "HEAD"],
                             capture_output=True, text=True).stdout.strip()

        out = subprocess.run(
            [binary, "metrics", str(dest), "--config", cfg, "--scope", "production", "--format", "json"],
            capture_output=True, text=True, timeout=timeout,
        )
        if out.returncode not in (0, 1) or not out.stdout.strip():
            return {**base, "ok": False, "error": "metrics: " + (out.stderr.strip()[:200] or f"rc={out.returncode}")}

        data = json.loads(out.stdout)
        prod = data.get("profiles", {}).get("production", {})
        tp = data.get("test_proxies", {})
        rec = {**base, "ok": True, "sha": sha}
        flatten("m.", prod, rec)
        flatten("tp.", tp, rec)
        return rec
    except subprocess.TimeoutExpired:
        return {**base, "ok": False, "error": f"timeout >{timeout}s"}
    except Exception as exc:  # never let one repo sink the run
        return {**base, "ok": False, "error": f"{type(exc).__name__}: {exc}"[:200]}
    finally:
        shutil.rmtree(dest, ignore_errors=True)

_PATH = os.environ.get("PATH", "/usr/bin:/bin")


def load_done(path: Path, retry_failed: bool) -> set:
    if not path.exists():
        return set()
    done = set()
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        if r.get("ok") or not retry_failed:
            done.add(r["full_name"])
    return done


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--frame", default=str(FRAME))
    ap.add_argument("--out", default=str(FEATURES))
    ap.add_argument("--bin", default=DEFAULT_BIN)
    ap.add_argument("--config", default=DEFAULT_CFG)
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--timeout", type=int, default=300, help="per-repo clone/measure timeout (s)")
    ap.add_argument("--max-kb", type=int, default=300000, help="skip repos larger than this (KB)")
    ap.add_argument("--limit", type=int, default=0, help="measure at most N repos this run (0=all)")
    ap.add_argument("--retry-failed", action="store_true", help="re-attempt rows previously recorded ok:false")
    args = ap.parse_args()

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    CHECKOUTS.mkdir(parents=True, exist_ok=True)

    rows = [json.loads(l) for l in Path(args.frame).read_text().splitlines() if l.strip()]
    done = load_done(out_path, args.retry_failed)
    todo = [r for r in rows if r["full_name"] not in done]
    if args.limit:
        todo = todo[: args.limit]
    print(f"frame={len(rows)} done={len(done)} todo={len(todo)} workers={args.workers}", flush=True)

    t0 = time.time()
    n_ok = n_fail = 0
    with out_path.open("a") as fh, ThreadPoolExecutor(max_workers=args.workers) as pool:
        futs = {pool.submit(measure_one, r, args.bin, args.config, args.timeout, args.max_kb): r
                for r in todo}
        for i, fut in enumerate(as_completed(futs), 1):
            rec = fut.result()
            with _write_lock:
                fh.write(json.dumps(rec) + "\n")
                fh.flush()
            if rec.get("ok"):
                n_ok += 1
            else:
                n_fail += 1
            if i % 25 == 0 or i == len(todo):
                rate = i / max(time.time() - t0, 1e-9)
                eta = (len(todo) - i) / max(rate, 1e-9)
                print(f"  {i}/{len(todo)}  ok={n_ok} fail={n_fail}  {rate:.1f}/s  eta {eta/60:.0f}m",
                      flush=True)
    print(f"\n=== ok={n_ok} fail={n_fail} in {(time.time()-t0)/60:.1f}m -> {out_path} ===")


if __name__ == "__main__":
    main()
