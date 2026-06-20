#!/usr/bin/env python3
"""Compare the slop and clean cohorts from the raw sloplint results and write report.md.

Two halves:
  1. VALIDATION — for each shipped rule, findings per KLOC, slop vs clean. A rule that
     discriminates fires far more on slop. A rule that fires equally on both is noise.
  2. DISCOVERY — raw per-function feature distributions (loc, cyclomatic, cognitive,
     nesting, params, file comment density) the linter has NO rule for yet. A feature
     whose cohort medians separate cleanly is a candidate for a new rule.

The separation ratio (slop median / clean median) ranks both. Stdlib only.
"""

from __future__ import annotations

import json
import statistics
from collections import defaultdict
from pathlib import Path

BENCH = Path(__file__).resolve().parent
RAW = BENCH / "results" / "raw"
REPORT = BENCH / "results" / "report.md"

# Per-function numeric features mined for the discovery half.
FEATURES = ["loc", "cyclomatic", "cognitive", "max_nesting", "params"]


def load_repos() -> list[dict]:
    manifest = json.loads((BENCH / "results" / "run_manifest.json").read_text())
    repos = []
    for entry in manifest:
        name = entry["name"]
        metrics = json.loads((RAW / f"{name}.metrics.json").read_text())
        findings = json.loads((RAW / f"{name}.check.json").read_text()).get("findings", [])
        functions = [
            json.loads(line)
            for line in (RAW / f"{name}.functions.jsonl").read_text().splitlines()
            if line.strip()
        ]
        repos.append({**entry, "metrics": metrics, "findings": findings, "functions": functions})
    return repos


def per_kloc(repo: dict, code: str | None = None) -> float:
    loc = repo["metrics"].get("total_loc", 0)
    if loc == 0:
        return 0.0
    n = sum(1 for f in repo["findings"] if code is None or f["code"] == code)
    return n / loc * 1000.0


def median(values: list[float]) -> float:
    return statistics.median(values) if values else 0.0


def ratio(slop: float, clean: float) -> float:
    if clean == 0:
        return float("inf") if slop > 0 else 0.0
    return slop / clean


def fmt_ratio(r: float) -> str:
    if r == float("inf"):
        return "∞"
    return f"{r:.2f}×"


def validation_table(by_cohort: dict[str, list[dict]]) -> list[str]:
    codes = sorted({f["code"] for r in by_cohort["slop"] + by_cohort["clean"] for f in r["findings"]})
    rows = []
    for code in codes:
        slop = median([per_kloc(r, code) for r in by_cohort["slop"]])
        clean = median([per_kloc(r, code) for r in by_cohort["clean"]])
        rows.append((code, slop, clean, ratio(slop, clean)))
    rows.sort(key=lambda x: x[3], reverse=True)

    out = ["## Validation — findings per KLOC (rules we already ship)", ""]
    out.append("| rule | slop /KLOC | clean /KLOC | separation |")
    out.append("| --- | ---: | ---: | ---: |")
    out.append(f"| **all rules** | {median([per_kloc(r) for r in by_cohort['slop']]):.2f} "
               f"| {median([per_kloc(r) for r in by_cohort['clean']]):.2f} "
               f"| {fmt_ratio(ratio(median([per_kloc(r) for r in by_cohort['slop']]), median([per_kloc(r) for r in by_cohort['clean']])))} |")
    for code, slop, clean, r in rows:
        out.append(f"| {code} | {slop:.2f} | {clean:.2f} | {fmt_ratio(r)} |")
    out += ["", "_Separation = slop median / clean median. >1 means the rule discriminates._", ""]
    return out


def repo_feature_medians(repo: dict) -> dict[str, float]:
    fns = repo["functions"]
    stats = {feat: median([f[feat] for f in fns]) for feat in FEATURES}
    # Tail rates: fraction of functions past a smell-ish threshold — tails separate cohorts
    # better than medians when most functions are small in both.
    n = len(fns) or 1
    stats["frac_cognitive_gt_15"] = sum(1 for f in fns if f["cognitive"] > 15) / n
    stats["frac_nesting_gt_3"] = sum(1 for f in fns if f["max_nesting"] > 3) / n
    stats["frac_loc_gt_50"] = sum(1 for f in fns if f["loc"] > 50) / n
    stats["frac_params_gt_5"] = sum(1 for f in fns if f["params"] > 5) / n
    stats["file_comment_density"] = median([f["file_comment_density"] for f in fns])
    return stats


def discovery_table(by_cohort: dict[str, list[dict]]) -> list[str]:
    keys = FEATURES + [
        "frac_cognitive_gt_15",
        "frac_nesting_gt_3",
        "frac_loc_gt_50",
        "frac_params_gt_5",
        "file_comment_density",
    ]
    slop = [repo_feature_medians(r) for r in by_cohort["slop"]]
    clean = [repo_feature_medians(r) for r in by_cohort["clean"]]

    rows = []
    for k in keys:
        s = median([m[k] for m in slop])
        c = median([m[k] for m in clean])
        rows.append((k, s, c, ratio(s, c)))
    rows.sort(key=lambda x: x[3], reverse=True)

    out = ["## Discovery — raw per-function features (no rule yet)", ""]
    out.append("| feature (cohort median of per-repo medians) | slop | clean | separation |")
    out.append("| --- | ---: | ---: | ---: |")
    for k, s, c, r in rows:
        out.append(f"| {k} | {s:.3f} | {c:.3f} | {fmt_ratio(r)} |")
    out += [
        "",
        "_Features at the top separate the cohorts most — candidates for a new rule._",
        "_`frac_*` rows are the share of functions past a smell threshold (tails matter more than medians)._",
        "",
    ]
    return out


def main() -> None:
    repos = load_repos()
    by_cohort = {"slop": [r for r in repos if r["cohort"] == "slop"],
                 "clean": [r for r in repos if r["cohort"] == "clean"]}
    if not by_cohort["slop"] or not by_cohort["clean"]:
        raise SystemExit("need at least one repo in each cohort — run run.py with real slop repos first")

    lines = ["# sloplint benchmark report", ""]
    lines.append(f"Cohorts: **{len(by_cohort['slop'])} slop** "
                 f"({', '.join(r['name'] for r in by_cohort['slop'])}) vs "
                 f"**{len(by_cohort['clean'])} clean** "
                 f"({', '.join(r['name'] for r in by_cohort['clean'])}).")
    lines.append("")
    lines += validation_table(by_cohort)
    lines += discovery_table(by_cohort)

    REPORT.write_text("\n".join(lines) + "\n")
    print(f"wrote {REPORT}")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
