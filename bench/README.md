# sloplint benchmark

Runs sloplint over a curated set of real repos and compares two cohorts to (1) **validate**
that the shipped rules actually fire more on low-quality code and (2) **discover** raw
code features that separate the cohorts — the seeds for new rules.

> **Cohorts label quality, not authorship.** `clean` = high-quality, widely-respected
> Python; `slop` = low-quality / vibe-coded Python that is dense with smells. Vibe-coded
> repos are just a convenient *source* of slop. Any rule mined from this data must still
> flag **badness**, never AI authorship — that is the whole point of sloplint.

## Layout

| File | Role |
| --- | --- |
| `repos.json` | the manifest: per-repo url, pinned `ref`, cohort, Python `paths`, measured metrics |
| `find_slop_gharchive.py` | discover candidate slop repos from GH Archive + score whole-history AI fraction (see its docstring) |
| `run.py` | clone each repo @ ref → run `sloplint check` + `metrics` → raw JSON in `results/raw/` |
| `analyze.py` | distill `results/raw/` into `results/report.md` (validation + discovery tables) |
| `results/run_manifest.json` | resolved commit SHAs + per-repo counts (committed; makes a run reproducible) |
| `results/report.md` | the distilled comparison (committed) |
| `checkouts/` | cloned trees — git-ignored, reproduce from the manifest |

## Usage

```bash
# Optional: grow the slop[] cohort. Streams GH Archive hours (no auth), scores via the
# core API, writes ranked candidates to results/. Promote good ones into repos.json.
python3 bench/find_slop_gharchive.py --hours 2026-06-15-12 2026-06-15-13 ...

python3 bench/run.py            # clone + measure everything (builds sloplint --release once)
python3 bench/run.py --only flask,rich   # subset
python3 bench/analyze.py        # write + print results/report.md
```

All three scripts are stdlib-only (Python 3.8+); the finder also shells out to `gh` + `curl`.
`run.py` builds the release binary with `cargo`, so a Rust toolchain must be on PATH.

## What the report shows

**Validation** — for every shipped rule, findings per KLOC in each cohort and the
`separation = slop median / clean median`. A rule with separation ≫ 1 discriminates; a rule
near 1 is firing on everything and needs tightening.

**Discovery** — per-function feature distributions sloplint has **no rule for** (function
LOC, cyclomatic/cognitive complexity, nesting, params, file comment density), plus tail
rates like "share of functions with cognitive > 15". Features whose cohort medians separate
cleanly are the strongest candidates for the next rule; confirm by eyeballing the slop
checkouts before writing one.

Per-function rows come from `sloplint metrics --format functions` (JSONL).

## Caveats

- **Normalize, always.** Raw finding counts scale with repo size; the report uses per-KLOC
  and per-function rates. Don't compare raw totals.
- **Pin refs.** Branches move; pin a tag or commit sha in `repos.json` so a run reproduces.
- **Pure-Python paths.** Point `paths` at Python-only dirs. Mixed C/Cython repos (numpy,
  torch) still work but the Python subset is noisier and less representative.
- A handful of repos per cohort is a smoke test, not a study. Medians over ~7+ repos per
  side are far more trustworthy than over 2.
