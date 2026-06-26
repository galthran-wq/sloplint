# Independent axes of the SQM panel — factor structure (N=5675)

Panel = 18 continuous metrics. PCA on the standardized panel; the primary pass residualizes each metric on log(LOC) so the axes are *quality beyond size*.


## Size-residualized (the independent quality axes)

Scree — eigenvalue = # of original metrics' worth of variance a PC captures (>1 = a real axis, Kaiser):

| PC | eigenvalue | % var | cumulative % |
|---|---:|---:|---:|
| PC1 | 3.02 | 17% | 17% ←Kaiser |
| PC2 | 1.81 | 10% | 27% ←Kaiser |
| PC3 | 1.64 | 9% | 36% ←Kaiser |
| PC4 | 1.37 | 8% | 44% ←Kaiser |
| PC5 | 1.13 | 6% | 50% ←Kaiser |
| PC6 | 1.05 | 6% | 56% ←Kaiser |
| PC7 | 1.03 | 6% | 61% ←Kaiser |
| PC8 | 0.99 | 5% | 67% |

**7 axes** (Kaiser). Varimax-rotated loadings (|·|≥0.35 shown; sign = direction):

| metric | F1 | F2 | F3 | F4 | F5 | F6 | F7 |
|---|---|---|---|---|---|---|---|
| avg_cog | -0.93 |  |  |  |  |  |  |
| max_cog |  | -0.90 |  |  |  |  |  |
| avg_cyc | -0.91 |  |  |  |  |  |  |
| max_cyc |  | -0.91 |  |  |  |  |  |
| avg_fn_loc |  |  |  |  |  | +0.40 |  |
| max_nest | -0.70 |  |  |  |  |  |  |
| god_units |  | -0.39 | +0.64 |  |  |  |  |
| clone |  |  | -0.43 |  |  |  | -0.37 |
| toplevel |  |  |  |  |  |  | -0.61 |
| comments |  |  |  |  |  |  | -0.65 |
| docstr |  |  |  |  | -0.79 |  |  |
| type_cov |  |  |  |  | -0.71 |  |  |
| propagation |  |  |  |  |  | +0.69 |  |
| cycles |  |  | +0.79 |  |  |  |  |
| modular_gap |  |  |  |  |  | -0.70 |  |
| test_code |  |  |  |  |  |  |  |
| assert_dens |  |  |  | +0.79 |  |  |  |
| assert_free |  |  |  | -0.72 |  |  |  |

Factor make-up (top metrics by |loading|):
- **F1** = avg_cog(-0.93), avg_cyc(-0.91), max_nest(-0.70)
- **F2** = max_cyc(-0.91), max_cog(-0.90), god_units(-0.39)
- **F3** = cycles(+0.79), god_units(+0.64), clone(-0.43)
- **F4** = assert_dens(+0.79), assert_free(-0.72)
- **F5** = docstr(-0.79), type_cov(-0.71)
- **F6** = modular_gap(-0.70), propagation(+0.69), avg_fn_loc(+0.40)
- **F7** = comments(-0.65), toplevel(-0.61), clone(-0.37)

## Raw (size kept in — shows size's role)

Scree — eigenvalue = # of original metrics' worth of variance a PC captures (>1 = a real axis, Kaiser):

| PC | eigenvalue | % var | cumulative % |
|---|---:|---:|---:|
| PC1 | 3.47 | 19% | 19% ←Kaiser |
| PC2 | 1.89 | 10% | 30% ←Kaiser |
| PC3 | 1.55 | 9% | 38% ←Kaiser |
| PC4 | 1.38 | 8% | 46% ←Kaiser |
| PC5 | 1.21 | 7% | 53% ←Kaiser |
| PC6 | 1.03 | 6% | 58% ←Kaiser |
| PC7 | 1.00 | 6% | 64% |
| PC8 | 0.98 | 5% | 69% |

**6 axes** (Kaiser). Varimax-rotated loadings (|·|≥0.35 shown; sign = direction):

| metric | F1 | F2 | F3 | F4 | F5 | F6 |
|---|---|---|---|---|---|---|
| avg_cog |  | -0.94 |  |  |  |  |
| max_cog | -0.84 |  |  |  |  |  |
| avg_cyc |  | -0.93 |  |  |  |  |
| max_cyc | -0.82 |  |  |  |  |  |
| avg_fn_loc |  |  |  |  |  |  |
| max_nest | -0.42 | -0.47 | +0.50 |  |  |  |
| god_units | -0.75 |  |  |  |  |  |
| clone |  |  | +0.46 |  |  |  |
| toplevel |  |  |  |  |  | -0.40 |
| comments |  |  |  |  |  | -0.68 |
| docstr |  |  |  |  | +0.82 |  |
| type_cov |  |  |  |  | +0.66 | +0.38 |
| propagation |  |  | -0.78 |  |  |  |
| cycles | -0.66 |  |  |  |  |  |
| modular_gap |  |  | +0.73 |  |  |  |
| test_code |  |  |  |  |  |  |
| assert_dens |  |  |  | -0.75 |  |  |
| assert_free |  |  |  | +0.75 |  |  |

Factor make-up (top metrics by |loading|):
- **F1** = max_cog(-0.84), max_cyc(-0.82), god_units(-0.75), cycles(-0.66)
- **F2** = avg_cog(-0.94), avg_cyc(-0.93), max_nest(-0.47)
- **F3** = propagation(-0.78), modular_gap(+0.73), max_nest(+0.50), clone(+0.46)
- **F4** = assert_free(+0.75), assert_dens(-0.75)
- **F5** = docstr(+0.82), type_cov(+0.66)
- **F6** = comments(-0.68), toplevel(-0.40), type_cov(+0.38), test_code(-0.34)

## Read

- The quality panel has **~7 independent axes** (size-residualized, Kaiser). A single scalar index blends them and loses *which* axis is bad — so report the axis profile; a one-number badge is only an optional rollup of it.
- Each axis' loadings give **data-driven weights** (no hand-guessing): within an axis, weight metrics by |loading|; across axes, they're ~orthogonal by construction.
