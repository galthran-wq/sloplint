#!/usr/bin/env python3
"""Run sloplint over every repo in the benchmark manifest and collect raw results.

For each repo it:
  1. shallow-clones the pinned ref into bench/checkouts/<name>/ (skipped if present),
  2. records the resolved commit SHA (so a run is reproducible even from a moving branch),
  3. runs `sloplint check` (findings) and `sloplint metrics` (aggregate + per-function rows),
  4. writes the raw JSON into bench/results/raw/.

Stdlib only (Python 3.8+). The cohort comparison lives in analyze.py.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

BENCH = Path(__file__).resolve().parent
REPO_ROOT = BENCH.parent
CHECKOUTS = BENCH / "checkouts"
RAW = BENCH / "results" / "raw"


def load_manifest(path: Path) -> dict:
    data = json.loads(path.read_text())
    return {"clean": data.get("clean", []), "slop": data.get("slop", [])}


def is_template(repo: dict) -> bool:
    return "REPLACE-ME" in repo["name"] or "OWNER/REPO" in repo["url"]


def build_sloplint() -> Path:
    """Build the release binary once and return its path."""
    print("building sloplint (release)…", flush=True)
    subprocess.run(
        ["cargo", "build", "--release", "-p", "sloplint"],
        cwd=REPO_ROOT,
        check=True,
    )
    binary = REPO_ROOT / "target" / "release" / "sloplint"
    if not binary.exists():
        sys.exit(f"sloplint binary not found at {binary}")
    return binary


def clone(repo: dict) -> Path:
    dest = CHECKOUTS / repo["name"]
    if dest.exists():
        print(f"  {repo['name']}: already checked out", flush=True)
        return dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    ref = repo.get("ref", "HEAD")
    print(f"  {repo['name']}: cloning {repo['url']}@{ref}", flush=True)

    def shallow_fetch(target: str) -> bool:
        """init + shallow fetch a single ref/SHA + checkout. GitHub allows fetch-by-SHA.

        Works for SHAs, tags, and branches without downloading full history — important
        for large pinned repos (e.g. SAGE ~3GB). Returns False so the caller can fall back."""
        try:
            subprocess.run(["git", "init", "-q", str(dest)], check=True)
            subprocess.run(["git", "-C", str(dest), "remote", "add", "origin", repo["url"]], check=True)
            subprocess.run(
                ["git", "-C", str(dest), "fetch", "--depth", "1", "origin", target],
                check=True, capture_output=True, text=True,
            )
            subprocess.run(["git", "-C", str(dest), "checkout", "-q", "FETCH_HEAD"], check=True)
            return True
        except subprocess.CalledProcessError:
            return False

    # Prefer a shallow single-ref fetch (handles pinned SHAs cheaply); fall back to a
    # branch shallow-clone, then to a full clone + checkout if the server rejects both.
    if shallow_fetch(ref):
        return dest
    shutil.rmtree(dest, ignore_errors=True)
    try:
        subprocess.run(
            ["git", "clone", "--depth", "1", "--branch", ref, repo["url"], str(dest)],
            check=True,
            capture_output=True,
            text=True,
        )
    except subprocess.CalledProcessError:
        shutil.rmtree(dest, ignore_errors=True)
        subprocess.run(["git", "clone", repo["url"], str(dest)], check=True)
        subprocess.run(["git", "-C", str(dest), "checkout", ref], check=True)
    return dest


def resolved_sha(checkout: Path) -> str:
    out = subprocess.run(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return out.stdout.strip()


def run_sloplint(binary: Path, args: list[str]) -> str:
    # check exits 1 on findings, which is not a failure for us — capture regardless.
    out = subprocess.run([str(binary), *args], capture_output=True, text=True)
    if out.returncode not in (0, 1):
        sys.stderr.write(out.stderr)
        raise RuntimeError(f"sloplint {args} failed ({out.returncode})")
    return out.stdout


def measure(binary: Path, repo: dict, checkout: Path) -> dict:
    targets = [str(checkout / p) for p in repo.get("paths", ["."])]
    name = repo["name"]

    # Pin the rule set via the benchmark config so cloned repos' own sloplint.toml (if any)
    # is ignored and noise rules (SLP010 comment-ban, SLP050 ASCII) are excluded.
    cfg = str(BENCH / "sloplint.bench.toml")
    check = run_sloplint(binary, ["check", *targets, "--preview", "--config", cfg, "--format", "json"])
    metrics = run_sloplint(binary, ["metrics", *targets, "--format", "json"])
    functions = run_sloplint(binary, ["metrics", *targets, "--format", "functions"])

    (RAW / f"{name}.check.json").write_text(check)
    (RAW / f"{name}.metrics.json").write_text(metrics)
    (RAW / f"{name}.functions.jsonl").write_text(functions)

    findings = json.loads(check).get("findings", []) if check.strip() else []
    return {
        "name": name,
        "cohort": repo["cohort"],
        "url": repo["url"],
        "ref": repo.get("ref"),
        "sha": resolved_sha(checkout),
        "paths": repo.get("paths", ["."]),
        "findings": len(findings),
        "functions": sum(1 for line in functions.splitlines() if line.strip()),
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--manifest", default=str(BENCH / "repos.json"))
    ap.add_argument("--only", help="comma-separated repo names to run (default: all)")
    ap.add_argument(
        "--fresh", action="store_true", help="delete checkouts before cloning"
    )
    args = ap.parse_args()

    if args.fresh and CHECKOUTS.exists():
        shutil.rmtree(CHECKOUTS)
    RAW.mkdir(parents=True, exist_ok=True)

    manifest = load_manifest(Path(args.manifest))
    wanted = set(args.only.split(",")) if args.only else None

    repos = []
    for cohort in ("clean", "slop"):
        for repo in manifest[cohort]:
            repo = {**repo, "cohort": cohort}
            if wanted and repo["name"] not in wanted:
                continue
            if is_template(repo):
                print(f"  skipping template entry {repo['name']} — replace it in repos.json")
                continue
            repos.append(repo)

    if not repos:
        sys.exit("no runnable repos (all slop entries are still templates?)")

    binary = build_sloplint()
    summary = []
    for repo in repos:
        try:
            checkout = clone(repo)
            summary.append(measure(binary, repo, checkout))
            print(f"  {repo['name']}: {summary[-1]['findings']} findings, "
                  f"{summary[-1]['functions']} functions", flush=True)
        except Exception as exc:  # keep going; one bad repo shouldn't sink the run
            print(f"  {repo['name']}: ERROR {exc}", file=sys.stderr)

    manifest_out = BENCH / "results" / "run_manifest.json"
    manifest_out.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"\nwrote {manifest_out} ({len(summary)} repos)")
    print("next: python3 bench/analyze.py")


if __name__ == "__main__":
    main()
