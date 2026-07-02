# SQM metrics vs external quality / provenance labels

N = 10585 (features ∩ labels, loc>=50). has_ci=7505 (71%), engineered=4340 (41%), ai_authored=3061 (29%), ai_heavy≥50%=411 (4%).

Partial = Spearman(metric, label | log LOC). Provenance (ai/claude) is a covariate, not a quality verdict.


## 1. Do AI-authored repos differ in SQM metrics? (the central covariate)

Partial Spearman of each metric vs ai_share / claude_share, controlling log(LOC):

| metric | vs ai_share | vs claude_share |
|---|---|---|
| avg_cognitive | +0.06 | +0.08 |
| max_cognitive | +0.02 | +0.04 |
| avg_cyclomatic | +0.09 | +0.11 |
| max_cyclomatic | +0.05 | +0.06 |
| avg_function_loc | +0.08 | +0.08 |
| max_nesting | +0.02 | +0.03 |
| god_units | +0.03 | +0.05 |
| clone_ratio | -0.09 | -0.08 |
| toplevel_ratio | -0.03 | -0.04 |
| comment_density | -0.01 | -0.00 |
| docstring_cov | +0.17 | +0.14 |
| type_cov | +0.19 | +0.16 |
| propagation | +0.04 | +0.04 |
| cycle_tangles | +0.07 | +0.05 |
| modularity_gap | +0.01 | +0.02 |
| test_code_ratio | +0.27 | +0.23 |
| assertion_density | +0.00 | -0.00 |
| assertion_free_rate | -0.07 | -0.08 |

AI-authored vs non-AI metric medians, **within each size bucket** (controls size):

| bucket | n_ai / n | avg_cognitive | max_cognitive | clone_ratio | test_code_ratio | assertion_free_rate | avg_ratio | total | max_nesting |
|---|---|---|---|---|---|---|---|---|---|
| s0 | 199/1558 | 4.44/3.75 | 46.00/20.00 | 0.09/0.00 | 0.07/0.00 | 0.01/0.01 | 0.05/0.06 | 1.00/0.00 | 5.00/4.00 |
| s1_9 | 253/1572 | 3.90/3.58 | 54.00/25.00 | 0.10/0.05 | 0.28/0.00 | 0.03/0.03 | 0.06/0.06 | 2.00/0.00 | 5.00/4.00 |
| s10_49 | 427/1635 | 3.71/3.55 | 72.00/40.00 | 0.10/0.08 | 0.53/0.03 | 0.04/0.04 | 0.07/0.08 | 5.00/1.00 | 6.00/5.00 |
| s50_199 | 619/1658 | 3.70/3.42 | 67.00/47.00 | 0.10/0.09 | 0.42/0.06 | 0.04/0.05 | 0.07/0.08 | 4.00/1.00 | 6.00/5.00 |
| s200_999 | 664/1651 | 3.90/3.51 | 85.00/52.00 | 0.11/0.10 | 0.40/0.07 | 0.05/0.05 | 0.07/0.07 | 7.00/2.00 | 6.50/6.00 |
| s1k_4999 | 565/1681 | 4.40/3.39 | 99.00/62.00 | 0.10/0.12 | 0.28/0.01 | 0.04/0.06 | 0.07/0.07 | 9.00/3.00 | 7.00/6.00 |
| s5k_up | 334/830 | 4.60/3.56 | 117.50/70.00 | 0.11/0.12 | 0.33/0.00 | 0.04/0.05 | 0.06/0.07 | 14.50/4.00 | 7.00/6.00 |

_(cells = AI-authored median / non-AI median; same size bucket → size-matched)_

## 2. Metrics vs quality/health labels (partial | log LOC) + AUROC

| metric | ai_share | claude_share | bugfix_ratio | log(contributors) | recent_authors | log(merged_PRs) | log(releases) | commits/wk | active_wk_frac | reviewed_rate | AUROC(has_ci) | AUROC(engineered) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| avg_cognitive | +0.06 | +0.08 | -0.01 | -0.09 | -0.11 | -0.17 | -0.10 | +0.15 | +0.11 | -0.10 | 0.47 | 0.46 |
| max_cognitive | +0.02 | +0.04 | +0.05 | -0.04 | -0.04 | -0.12 | -0.05 | +0.05 | +0.01 | -0.09 | 0.50 | 0.63 |
| avg_cyclomatic | +0.09 | +0.11 | +0.00 | -0.11 | -0.12 | -0.16 | -0.10 | +0.18 | +0.13 | -0.10 | 0.48 | 0.46 |
| max_cyclomatic | +0.05 | +0.06 | +0.06 | -0.04 | -0.04 | -0.11 | -0.03 | +0.06 | +0.02 | -0.08 | 0.51 | 0.65 |
| avg_function_loc | +0.08 | +0.08 | +0.00 | -0.07 | -0.03 | -0.09 | -0.07 | +0.07 | +0.05 | -0.02 | 0.49 | 0.51 |
| max_nesting | +0.02 | +0.03 | +0.05 | -0.03 | -0.03 | -0.10 | -0.05 | +0.05 | +0.03 | -0.06 | 0.50 | 0.63 |
| god_units | +0.03 | +0.05 | +0.01 | -0.05 | -0.08 | -0.11 | -0.04 | +0.12 | +0.10 | -0.09 | 0.52 | 0.65 |
| clone_ratio | -0.09 | -0.08 | -0.10 | -0.07 | -0.03 | -0.10 | -0.10 | -0.13 | -0.13 | -0.07 | 0.45 | 0.56 |
| toplevel_ratio | -0.03 | -0.04 | -0.00 | +0.08 | +0.03 | +0.09 | +0.06 | -0.03 | -0.02 | +0.05 | 0.50 | 0.54 |
| comment_density | -0.01 | -0.00 | -0.04 | -0.02 | -0.02 | -0.12 | -0.13 | -0.05 | -0.04 | +0.00 | 0.43 | 0.44 |
| docstring_cov | +0.17 | +0.14 | +0.07 | +0.02 | +0.07 | +0.14 | +0.15 | -0.01 | +0.00 | +0.09 | 0.60 | 0.65 |
| type_cov | +0.19 | +0.16 | +0.14 | +0.07 | +0.11 | +0.24 | +0.21 | +0.04 | +0.05 | +0.12 | 0.63 | 0.69 |
| propagation | +0.04 | +0.04 | +0.03 | +0.04 | +0.00 | +0.08 | +0.20 | +0.08 | +0.11 | +0.04 | 0.56 | 0.42 |
| cycle_tangles | +0.07 | +0.05 | +0.08 | +0.09 | +0.05 | +0.17 | +0.17 | +0.08 | +0.10 | +0.09 | 0.59 | 0.67 |
| modularity_gap | +0.01 | +0.02 | +0.11 | +0.03 | +0.10 | +0.08 | +0.03 | -0.09 | -0.10 | +0.00 | 0.52 | 0.66 |
| test_code_ratio | +0.27 | +0.23 | +0.25 | +0.26 | +0.24 | +0.47 | +0.36 | +0.05 | +0.04 | +0.24 | 0.70 | 0.80 |
| assertion_density | +0.00 | -0.00 | -0.02 | +0.02 | -0.01 | +0.04 | +0.08 | +0.03 | +0.03 | +0.02 | 0.56 | 0.55 |
| assertion_free_rate | -0.07 | -0.08 | +0.03 | +0.16 | +0.16 | +0.12 | +0.07 | -0.15 | -0.08 | +0.13 | 0.52 | 0.56 |

## 3. AUROC(has_ci) within each star-bucket (is it more than size?)

| bucket | n | ci% | avg_cognitive | max_cognitive | clone_ratio | test_code_ratio | assertion_free_rate | avg_ratio |
|---|---|---|---|---|---|---|---|---|
| s0 | 1558 | 59% | 0.50 | 0.43 | 0.41 | 0.49 | 0.44 | 0.43 |
| s1_9 | 1572 | 62% | 0.45 | 0.40 | 0.41 | 0.57 | 0.55 | 0.52 |
| s10_49 | 1635 | 79% | 0.45 | 0.46 | 0.45 | 0.72 | 0.46 | 0.51 |
| s50_199 | 1658 | 82% | 0.42 | 0.45 | 0.45 | 0.74 | 0.56 | 0.53 |
| s200_999 | 1651 | 80% | 0.45 | 0.47 | 0.42 | 0.76 | 0.51 | 0.51 |
| s1k_4999 | 1681 | 65% | 0.50 | 0.56 | 0.43 | 0.76 | 0.47 | 0.52 |
| s5k_up | 830 | 66% | 0.53 | 0.62 | 0.46 | 0.80 | 0.52 | 0.51 |

## 4. Supervised: logistic(metric panel) → `engineered`, 5-fold CV

- N=5674, positive rate=64%, **CV AUROC = 0.643**

| panel feature | std weight |
|---|---|
| loc_log | +0.48 |
| m.packages.propagation_cost | +0.31 |
| m.god_units.total | -0.18 |
| m.duplication.clone_ratio | -0.15 |
| m.avg_cognitive | -0.14 |
| tp.assertion_free_rate | -0.14 |
| tp.test_code_ratio | +0.08 |
| m.top_level_code.avg_ratio | +0.07 |
| m.max_nesting | +0.03 |
| m.max_cognitive | +0.02 |
