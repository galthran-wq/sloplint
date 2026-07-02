# Alves/SIG quality profiles — full dataset (repos=11909, engineered=4387)

Thresholds = LOC-weighted 70/80/90% quantiles of code volume; bands low/mod/high/very-high.


## functions.jsonl — derived thresholds (70 / 80 / 90 %)

| metric | low ≤ | moderate ≤ | high ≤ | very-high > |
|---|--:|--:|--:|--:|
| unit complexity (cyclomatic) | 9 | 13 | 21 | 21 |
| unit complexity (cognitive) | 11 | 18 | 34 | 34 |
| unit size (function LOC) | 64 | 90 | 149 | 149 |
| unit interfacing (params) | 3 | 4 | 6 | 6 |
| nesting depth | 3 | 3 | 4 | 4 |

### Validation — median % of code in high+very-high bands (engineered vs non)

| metric | engineered | non-engineered | discriminates? |
|---|--:|--:|---|
| unit complexity (cyclomatic) | 14.6% | 13.6% | ⚠ eng worse |
| unit complexity (cognitive) | 15.0% | 14.3% | ⚠ eng worse |
| unit size (function LOC) | 17.6% | 12.7% | ⚠ eng worse |
| unit interfacing (params) | 16.7% | 8.0% | ⚠ eng worse |
| nesting depth | 10.6% | 11.6% | ✅ eng cleaner |

## classes.jsonl — derived thresholds (70 / 80 / 90 %)

| metric | low ≤ | moderate ≤ | high ≤ | very-high > |
|---|--:|--:|--:|--:|
| WMC | 55 | 85 | 155 | 155 |
| LCOM4 | 2 | 3 | 5 | 5 |
| CBO | 3 | 5 | 8 | 8 |
| NOM | 13 | 19 | 31 | 31 |
| DIT | 0 | 1 | 1 | 1 |

### Validation — median % of code in high+very-high bands (engineered vs non)

| metric | engineered | non-engineered | discriminates? |
|---|--:|--:|---|
| WMC | 10.8% | 0.0% | ⚠ eng worse |
| LCOM4 | 9.8% | 1.4% | ⚠ eng worse |
| CBO | 10.9% | 0.0% | ⚠ eng worse |
| NOM | 12.8% | 0.0% | ⚠ eng worse |
| DIT | 0.3% | 0.0% | ≈ flat |
