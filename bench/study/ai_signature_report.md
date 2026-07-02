# AI-authorship signature — deep dive

N=10585. non_ai=7524, ai_any(>0)=2650, ai_heavy(≥0.5)=411. ai_share = AI-tool commit-trailer fraction of the last 100 commits (lower bound).

_Provenance is a covariate, not a quality verdict — the panel measures construction._


## A. Cohort sizes by star bucket

| bucket | non_ai | ai_any | ai_heavy |
|---|--:|--:|--:|
| s0 | 1359 | 155 | 44 |
| s1_9 | 1319 | 200 | 53 |
| s10_49 | 1208 | 350 | 77 |
| s50_199 | 1039 | 541 | 78 |
| s200_999 | 987 | 596 | 68 |
| s1k_4999 | 1116 | 509 | 56 |
| s5k_up | 496 | 299 | 35 |

## B. ai_heavy vs non_ai — size-matched medians (heavy/non per bucket)

| bucket | n_heavy | max_cog | god_units | avg_cog | clone | test_code | type_cov | docstr |
|---|---|---|---|---|---|---|---|---|
| s0 | 44 | 65.50/20.00 | 3.00/0.00 | 6.63/3.75 | 0.11/0.00 | 0.01/0.00 | 0.84/0.11 | 0.59/0.17 |
| s1_9 | 53 | 65.00/25.00 | 6.00/0.00 | 4.63/3.58 | 0.10/0.05 | 0.32/0.00 | 0.93/0.33 | 0.62/0.26 |
| s10_49 | 77 | 123.00/40.00 | 17.00/1.00 | 4.77/3.55 | 0.11/0.08 | 0.63/0.03 | 0.91/0.61 | 0.65/0.39 |
| s50_199 | 78 | 96.50/47.00 | 12.00/1.00 | 4.78/3.42 | 0.11/0.09 | 0.63/0.06 | 0.95/0.72 | 0.70/0.39 |
| s200_999 | 68 | 114.50/52.00 | 25.50/2.00 | 4.65/3.51 | 0.10/0.10 | 0.65/0.07 | 0.91/0.67 | 0.64/0.39 |
| s1k_4999 | 56 | 178.00/62.00 | 23.50/3.00 | 6.00/3.39 | 0.09/0.12 | 0.29/0.01 | 0.88/0.52 | 0.67/0.32 |
| s5k_up | 35 | 102.00/70.00 | 14.00/4.00 | 5.89/3.56 | 0.10/0.12 | 0.37/0.00 | 0.91/0.48 | 0.66/0.26 |

## C. Partial Spearman vs ai_share / claude_share | log(LOC), with bootstrap 95% CI

| metric | vs ai_share [95% CI] | vs claude_share [95% CI] |
|---|---|---|
| max_cog | +0.02 [+0.01,+0.04] | +0.04 [+0.02,+0.06] |
| god_units | +0.03 [+0.01,+0.05] | +0.05 [+0.03,+0.07] |
| avg_cog | **+0.06** [+0.05,+0.08] | **+0.08** [+0.06,+0.10] |
| clone | **-0.09** [-0.10,-0.07] | **-0.08** [-0.10,-0.07] |
| test_code | **+0.27** [+0.25,+0.29] | **+0.23** [+0.21,+0.25] |
| type_cov | **+0.19** [+0.17,+0.20] | **+0.16** [+0.14,+0.18] |
| docstr | **+0.17** [+0.15,+0.18] | **+0.14** [+0.12,+0.15] |
| avg_cyc | **+0.09** [+0.08,+0.11] | **+0.11** [+0.09,+0.13] |
| toplevel | -0.03 [-0.04,-0.01] | -0.04 [-0.06,-0.02] |

## D. Complexity-tail rigor — ai_heavy vs non_ai (p50 / p90 / p95, %over)

Pooled over the well-measured mid buckets (s10_49…s1k_4999) to control size:

(ai_heavy n=279, non_ai n=4350)

| metric | grp | p50 | p90 | p95 | %over |
|---|---|--:|--:|--:|--:|
| max_cog (>50) | ai_heavy | 115 | 379 | 514 | 83% |
| max_cog (>50) | non_ai | 50 | 211 | 318 | 49% |
| god_units (>0) | ai_heavy | 16 | 78 | 143 | 88% |
| god_units (>0) | non_ai | 2 | 29 | 57 | 62% |
| max_cyc (>40) | ai_heavy | 61 | 181 | 233 | 72% |
| max_cyc (>40) | non_ai | 29 | 97 | 140 | 35% |

## E. Top AI-heavy repos by max_cognitive (grounding)

| repo | stars | max_cog | avg_cog | god_units | test_code | LOC |
|---|--:|--:|--:|--:|--:|--:|
| drussell23/JARVIS | 17 | 2915 | 4.6 | 1188 | 0.43 | 2295114 |
| safishamsi/graphify | 71824 | 1442 | 12.3 | 85 | 0.78 | 41435 |
| mslade50/New_Seasonals | 0 | 1424 | 14.6 | 55 | 0.08 | 44991 |
| microbiomedata/nmdc-schema | 48 | 1068 | 8.0 | 19 | 0.04 | 52120 |
| drandyhaas/KiCadRoutingTools | 208 | 1036 | 16.9 | 181 | 0.10 | 75829 |
| bagofwords1/bagofwords | 444 | 1034 | 7.1 | 213 | 0.33 | 167686 |
| gershuni/GenizahSearch | 13 | 1025 | 7.7 | 257 | 0.46 | 230043 |
| igerber/diff-diff | 271 | 856 | 10.6 | 187 | 1.29 | 134156 |
| youngbryan97/aura | 62 | 823 | 5.0 | 464 | 0.33 | 651725 |
| kstevica/captain-claw | 56 | 777 | 6.8 | 197 | 0.07 | 169253 |
| ralforion/orionbelt-ontology-builder | 118 | 614 | 20.9 | 26 | 0.24 | 9650 |
| isair/jarvis | 1260 | 611 | 6.8 | 37 | 1.07 | 40937 |
| Peuqui/AIfred-Intelligence | 32 | 604 | 5.2 | 96 | 0.06 | 124934 |
| BigBodyCobain/Shadowbroker | 9385 | 595 | 6.9 | 190 | 0.55 | 135343 |
| jykim82/project | 0 | 574 | 10.5 | 63 | 0.04 | 48664 |
