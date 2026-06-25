# SQM metrics vs repository signals — a 10k-repo study

Scaled validation of sloplint's software-quality-metric (SQM) panel against external repository
signals, on a stratified random sample of **~10.5k Python repos** (2021–2026). Continues the
hand-read ~150-repo cohort work in [#55](https://github.com/galthran-wq/sloplint/issues/55) and
the index-reweighting analysis in [#142](https://github.com/galthran-wq/sloplint/issues/142),
at ~70× the scale.

## Method

- **Sample** (`sample_repos.py` → `frame.jsonl`, N=12,049): stratified over star-bucket × created-
  window (7 buckets s0…s5k+, even ~1.8k/bucket; high buckets population-capped) via the Search API,
  proxies harvested from the search payload. Avoids the dead-zero-star swamp a uniform sample
  produces (no variance in the outcome).
- **Measure** (`measure_stream.py` → `features.jsonl`, 11,022 ok): streaming shallow-clone →
  `sloplint metrics --scope production --format json` → flatten the 118-field panel → reap the
  checkout (disk stays flat). ~8% drop (deleted repos, 300 MB size cap).
- **Label** (`fetch_labels.py` + `fetch_contributors.py` → `labels.jsonl`, `contributors.jsonl`):
  per-repo GitHub GraphQL/REST — process (has_ci, merged_prs, reviewed_rate, releases), defect
  (bugfix_ratio), team (contributors, recent_authors), cadence (commits_per_week, active_week_frac),
  and **provenance** (ai_share, claude_share from commit-trailer fraction).
- **Analyse** (`study.py`, `label_study.py` → `report.md`, `label_report.md`): Spearman, partial
  Spearman controlling log(LOC), AUROC, per-bucket bands, 5-fold-CV logistic. Stdlib stats only.

## Finding 1 — popularity is NOT a quality label (size is the whole story)

Controlling for size (log LOC), **every SQM metric is uncorrelated with stars/forks** — partial
Spearman ∈ [−0.04, +0.07] (N=10,586). The high *raw* AUROC of max-based metrics (max_cognitive 0.73,
god_units 0.72) is pure size confound: those metrics track LOC, and LOC tracks stars (bucket-median
LOC rises 682 → 14,040 from s0 to s5k+). `avg_cognitive` is **flat ~3.6–3.9 in every star bucket**.
Confirms #55's size-confound warning at scale and refutes the "regress metrics on stars → quality
bands" plan: **stars measure usefulness/marketing, not construction quality.**

## Finding 2 — engineering discipline DOES track the metrics (unlike popularity)

Against process/discipline labels the picture inverts — real, size-independent separation:

| metric | AUROC(has_ci) | AUROC(engineered) | notable partial \| LOC |
| --- | ---: | ---: | --- |
| **test_code_ratio** | **0.70** | **0.80** | +0.47 vs log(merged_PRs), +0.36 vs log(releases) |
| type_cov | 0.63 | 0.69 | +0.24 vs merged_PRs |
| cycle_tangles | 0.59 | 0.67 | |
| god_units | 0.52 | 0.65 | −0.11 vs merged_PRs |
| clone_ratio | 0.45 | 0.56 | −0.10…−0.13 (discipline → less duplication) |
| avg_cognitive | 0.47 | 0.46 | −0.17 vs merged_PRs, **+0.15 vs commits/week** |

`test_code_ratio` is the dominant size-independent signal (AUROC 0.72–0.80 *within* each size
bucket) — consistent with #55's "test-substance is the robust axis". Note avg complexity is *lower*
in team/PR/release-disciplined repos but *higher* in high-velocity (commits/week) repos: velocity ≠
discipline. **The metric set captures real engineering quality — popularity was just the wrong label.**

## Finding 3 — AI-authored repos have a distinct signature: clean surface, heavy tail

29% of the sample (3,061 repos) carry AI-tool commit trailers (a lower bound — undetected AI repos
dilute the contrast, so the true effect is *stronger*). Size-matched (AI-median / non-AI-median in
the same bucket), the pattern holds across all 7 buckets:

| metric | s10_49 | s200_999 | s5k_up | reading |
| --- | --- | --- | --- | --- |
| **max_cognitive** | 72 / 40 | 85 / 52 | 117 / 70 | AI ~1.6–2× heavier complexity tail |
| **test_code_ratio** | 0.53 / 0.03 | 0.40 / 0.07 | 0.33 / 0.00 | AI far MORE tested |
| **god_units** | 5 / 1 | 7 / 2 | 14 / 4 | AI more god-units |
| avg_cognitive | 3.71 / 3.55 | 3.90 / 3.51 | 4.60 / 3.56 | AI mildly more complex on average |

Partial vs ai_share | LOC: **+tests (+0.27), +types (+0.19), +docstrings (+0.17), −duplication
(−0.09)**, but **+avg complexity (+0.06…+0.09) and a ~2× max-complexity / god-unit tail.** AI writes
the surface discipline LLMs are good at (tests, type hints, docstrings) while producing more monster
functions — the project's **"clean surface, slop logic"** thesis, now empirical at N=10.5k. This is
a *signature*, not a verdict: slop is badness, not provenance — the panel measures construction, and
AI construction differs.

## Finding 4 — a supervised "engineered" model is weak and size-dominated

Logistic(metric panel) → `engineered` (has_ci ∧ merged_prs ∧ releases): **5-fold CV AUROC = 0.643**.
Real but modest, and carried by `loc_log` (+0.48); the quality metrics have correct signs
(god_units −0.18, clone_ratio −0.15, avg_cognitive −0.14, assertion_free_rate −0.14) but small
weights. The constructive deliverable is therefore **descriptive, not predictive**: the size-matched
percentile reference distribution over 10k real repos (to calibrate the slop_index z-scores) plus
the AI-signature above — not a single popularity/quality oracle.

## Caveats

- `ai_share` from last-100 commit trailers undercounts AI repos → a lower-bound contrast.
- `test_code_ratio` reads ~0 for some mega-repos with separate test layouts (glob mismatch),
  inflating the AI/discipline contrast in the top buckets; the clean mid-buckets show the same pattern.
- Within-cell sampling used `sort=updated` (mild recency bias); 300 MB size cap drops the largest
  repos (~8% measurement drop, skews slightly away from huge popular repos).
- All correlational; AI repos also differ in domain/age.

## Reproduce

```
python3 sample_repos.py      # frame.jsonl   (live GitHub — snapshot, not reproducible byte-for-byte)
python3 measure_stream.py    # features.jsonl  (needs the sloplint release binary)
python3 fetch_labels.py --workers 1 --batch 8   # labels.jsonl (batch>10 → HTTP 502)
python3 fetch_contributors.py                    # contributors.jsonl (slow, core-rate-limited)
python3 study.py && python3 label_study.py       # report.md, label_report.md
```

Datasets (`*.jsonl`) are gitignored — point-in-time snapshots of live GitHub, regenerated by the
scripts above. The committed deliverables are the scripts and the two reports.
