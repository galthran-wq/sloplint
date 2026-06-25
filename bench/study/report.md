# SQM metrics vs repo-popularity proxies — study report

N = 10586 repos (ok, total_loc >= 50). Generated from `features.jsonl`.

Partial = Spearman controlling for log(total_loc) (the dominant size confound).


## Partial Spearman (metric vs proxy | log LOC) + AUROC

| metric | log(stars+1) | log(forks+1) | age_days | -days_since_push | log(open_issues+1) | AUROC(stars) |
|---|---|---|---|---|---|---|
| avg_cognitive | -0.01 | -0.01 | -0.21 | +0.07 | -0.11 | 0.52 |
| max_cognitive | +0.05 | +0.04 | -0.12 | -0.02 | -0.04 | 0.73 |
| avg_cyclomatic | -0.03 | -0.03 | -0.25 | +0.10 | -0.13 | 0.52 |
| max_cyclomatic | +0.05 | +0.04 | -0.14 | -0.01 | -0.04 | 0.74 |
| avg_function_loc | -0.01 | -0.00 | -0.18 | +0.04 | -0.04 | 0.54 |
| max_nesting | +0.06 | +0.06 | -0.12 | -0.02 | -0.02 | 0.72 |
| god_units | +0.01 | +0.00 | -0.12 | +0.04 | -0.06 | 0.72 |
| clone_ratio | -0.02 | -0.01 | +0.07 | -0.08 | -0.04 | 0.63 |
| toplevel_ratio | -0.00 | +0.00 | +0.09 | -0.02 | +0.05 | 0.52 |
| comment_density | -0.01 | +0.00 | -0.03 | -0.01 | +0.00 | 0.50 |
| docstring_cov | -0.04 | -0.04 | -0.02 | +0.09 | +0.02 | 0.55 |
| type_cov | +0.03 | +0.01 | -0.08 | +0.07 | +0.05 | 0.60 |
| propagation | +0.03 | +0.01 | +0.05 | +0.05 | +0.02 | 0.34 |
| cycle_tangles | +0.03 | +0.03 | +0.06 | +0.06 | +0.07 | 0.65 |
| modularity_gap | +0.07 | +0.06 | +0.01 | -0.09 | +0.07 | 0.70 |
| test_code_ratio | +0.04 | +0.05 | +0.06 | +0.09 | +0.20 | 0.65 |
| assertion_density | +0.00 | -0.01 | -0.01 | +0.03 | +0.01 | 0.53 |
| assertion_free_rate | +0.05 | +0.07 | +0.17 | -0.08 | +0.14 | 0.58 |

## Headline partials with bootstrap 95% CI (vs log stars | log LOC)

| metric | partial rho | 95% CI |
|---|---|---|
| avg_cognitive | -0.01 | [-0.03, +0.01] |
| max_cognitive | +0.05 | [+0.03, +0.07] |
| avg_cyclomatic | -0.03 | [-0.04, -0.01] |
| max_cyclomatic | +0.05 | [+0.03, +0.07] |
| avg_function_loc | -0.01 | [-0.03, +0.01] |
| max_nesting | +0.06 | [+0.05, +0.08] |
| god_units | +0.01 | [-0.01, +0.03] |
| clone_ratio | -0.02 | [-0.04, +0.00] |
| toplevel_ratio | -0.00 | [-0.02, +0.02] |
| comment_density | -0.01 | [-0.03, +0.01] |
| docstring_cov | -0.04 | [-0.06, -0.02] |
| type_cov | +0.03 | [+0.01, +0.05] |
| propagation | +0.03 | [+0.01, +0.05] |
| cycle_tangles | +0.03 | [+0.02, +0.05] |
| modularity_gap | +0.07 | [+0.05, +0.08] |
| test_code_ratio | +0.04 | [+0.02, +0.06] |
| assertion_density | +0.00 | [-0.02, +0.03] |
| assertion_free_rate | +0.05 | [+0.02, +0.08] |

## Per-star-bucket medians (band view)

| bucket | n | total_loc | avg_cognitive | max_cognitive | clone_ratio | test_code_ratio | assertion_free_rate | avg_ratio | total |
|---|---|---|---|---|---|---|---|---|---|
| s0 | 1558 | 682.00 | 3.80 | 23.00 | 0.04 | 0.00 | 0.01 | 0.06 | 0.00 |
| s1_9 | 1572 | 1304.00 | 3.63 | 28.00 | 0.06 | 0.00 | 0.03 | 0.06 | 0.00 |
| s10_49 | 1635 | 5036.00 | 3.61 | 47.00 | 0.09 | 0.12 | 0.04 | 0.07 | 1.00 |
| s50_199 | 1658 | 8049.00 | 3.55 | 54.00 | 0.09 | 0.20 | 0.04 | 0.07 | 2.00 |
| s200_999 | 1651 | 11270.00 | 3.65 | 66.00 | 0.10 | 0.19 | 0.05 | 0.07 | 4.00 |
| s1k_4999 | 1681 | 11057.00 | 3.65 | 72.00 | 0.11 | 0.05 | 0.04 | 0.07 | 4.00 |
| s5k_up | 831 | 14040.00 | 3.97 | 85.00 | 0.11 | 0.06 | 0.04 | 0.07 | 6.00 |
