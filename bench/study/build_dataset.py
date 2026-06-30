#!/usr/bin/env python3
"""Build the reusable research dataset: repos + functions + classes, all keyed by full_name.

Enriches the existing 12k frame with the per-entity feeds we were missing (needed for Alves/RTTOOL
threshold work). For each repo: shallow-clone → run sloplint three ways (aggregate panel, per-function,
per-class) → write three JSONL tables → reap the checkout. Merges the GraphQL labels + contributor
counts we already fetched. Resumable (skips repos already in repos.jsonl).

Outputs under dataset/:
  repos.jsonl     — one row/repo: identity + sha + sample-meta + proxies + labels + the 118-feature panel (flat, m.*/tp.*)
  functions.jsonl — one row/function: full_name, sha, file (repo-relative), + per-function metrics
  classes.jsonl   — one row/class:    full_name, sha, file, + full CK per-class metrics

Join any of them on full_name. Streaming clone→measure→reap keeps disk flat. Stdlib + git + release bin.
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
DATASET = STUDY / "dataset"
CO = STUDY / "co_ds"
BIN = "/disk1/sloplint/target/release/sloplint"
CFG = str(BENCH / "profiles.toml")
_lock = threading.Lock()

LABEL_FIELDS = ["has_ci", "merged_prs", "releases", "reviewed_rate", "bugfix_ratio",
                "recent_authors", "commits_total", "commits_per_week", "median_gap_days",
                "active_week_frac", "recent_span_days", "ai_share", "claude_share"]


def flatten(prefix, obj, out):
    for k, v in obj.items():
        key = f"{prefix}{k}"
        if isinstance(v, dict):
            flatten(key + ".", v, out)
        elif isinstance(v, (int, float, bool)) or v is None:
            out[key] = v


def sloplint(path, fmt):
    out = subprocess.run([BIN, "metrics", path, "--config", CFG, "--scope", "production",
                          "--format", fmt], capture_output=True, text=True, timeout=240)
    return out.stdout


def process(repo, labels, contribs, fh_r, fh_f, fh_c):
    name = repo["full_name"]
    dest = CO / name.replace("/", "__")
    shutil.rmtree(dest, ignore_errors=True)
    try:
        c = subprocess.run(["git", "clone", "--depth", "1", "--quiet", "--no-tags",
                            "--single-branch", f"https://github.com/{name}", str(dest)],
                           capture_output=True, timeout=240,
                           env={"GIT_TERMINAL_PROMPT": "0", "GIT_LFS_SKIP_SMUDGE": "1",
                                "PATH": "/usr/bin:/bin"})
        if c.returncode != 0:
            return _row(fh_r, {**_meta(repo, labels, contribs), "ok": False, "error": "clone"})
        sha = subprocess.run(["git", "-C", str(dest), "rev-parse", "HEAD"],
                             capture_output=True, text=True).stdout.strip()
        # aggregate panel
        data = json.loads(sloplint(str(dest), "json") or "{}")
        prod = data.get("profiles", {}).get("production", {})
        tp = data.get("test_proxies", {})
        row = {**_meta(repo, labels, contribs), "ok": True, "sha": sha}
        flatten("m.", prod, row)
        flatten("tp.", tp, row)
        # per-function + per-class
        prefix = str(dest) + "/"
        funcs = [json.loads(l) for l in sloplint(str(dest), "functions").splitlines() if l.strip()]
        klass = [json.loads(l) for l in sloplint(str(dest), "classes").splitlines() if l.strip()]
        with _lock:
            fh_r.write(json.dumps(row) + "\n"); fh_r.flush()
            for f in funcs:
                f["file"] = f.get("file", "").replace(prefix, "")
                fh_f.write(json.dumps({"full_name": name, "sha": sha, **f}) + "\n")
            for k in klass:
                k["file"] = k.get("file", "").replace(prefix, "")
                fh_c.write(json.dumps({"full_name": name, "sha": sha, **k}) + "\n")
            fh_f.flush(); fh_c.flush()
    except Exception as exc:
        _row(fh_r, {**_meta(repo, labels, contribs), "ok": False, "error": f"{type(exc).__name__}"[:60]})
    finally:
        shutil.rmtree(dest, ignore_errors=True)


def _meta(repo, labels, contribs):
    m = {"full_name": repo["full_name"], "url": f"https://github.com/{repo['full_name']}"}
    for k in ("stratum", "layer", "stars", "created_at", "pushed_at", "size_kb", "forks",
              "open_issues", "language", "archived"):
        m[k] = repo.get(k)
    lab = labels.get(repo["full_name"], {})
    for k in LABEL_FIELDS:
        m[k] = lab.get(k)
    m["contributors"] = contribs.get(repo["full_name"])
    return m


def _row(fh, row):
    with _lock:
        fh.write(json.dumps(row) + "\n"); fh.flush()


def load_kv(path, key="full_name"):
    out = {}
    if path.exists():
        for l in path.read_text().splitlines():
            if l.strip():
                r = json.loads(l)
                if r.get("ok", True):
                    out[r[key]] = r
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--frame", default=str(STUDY / "frame.jsonl"))
    ap.add_argument("--workers", type=int, default=14)
    ap.add_argument("--limit", type=int, default=0)
    args = ap.parse_args()

    DATASET.mkdir(exist_ok=True); CO.mkdir(exist_ok=True)
    labels = load_kv(STUDY / "labels.jsonl")
    contribs = {k: v.get("contributors") for k, v in load_kv(STUDY / "contributors.jsonl").items()}
    frame = [json.loads(l) for l in Path(args.frame).read_text().splitlines() if l.strip()]
    done = set()
    rp = DATASET / "repos.jsonl"
    if rp.exists():
        done = {json.loads(l)["full_name"] for l in rp.read_text().splitlines() if l.strip()}
    todo = [r for r in frame if r["full_name"] not in done]
    if args.limit:
        todo = todo[: args.limit]
    print(f"frame={len(frame)} done={len(done)} todo={len(todo)} "
          f"(labels={len(labels)}, contribs={len(contribs)})", flush=True)

    with (rp).open("a") as fh_r, (DATASET / "functions.jsonl").open("a") as fh_f, \
         (DATASET / "classes.jsonl").open("a") as fh_c, \
         ThreadPoolExecutor(max_workers=args.workers) as pool:
        futs = [pool.submit(process, r, labels, contribs, fh_r, fh_f, fh_c) for r in todo]
        for i, _ in enumerate(as_completed(futs), 1):
            if i % 100 == 0:
                print(f"  {i}/{len(todo)}", flush=True)
    shutil.rmtree(CO, ignore_errors=True)
    print(f"\n=== done. dataset/ written ===")


if __name__ == "__main__":
    main()
