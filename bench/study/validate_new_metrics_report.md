# New #265 metrics — do they discriminate quality? (size-controlled 8–30k LOC, N=1591)

Per-repo mean of the metric; hi = top tercile of label. ✅ good/mature side better · ⚠ worse · ≈ flat.

| metric (polarity) | contributors | has_ci | engineered | stars |
|---|---|---|---|---|
| rfc (hi, class) | 11.19/12.31✅ | 11.11/14.41✅ | 11.08/12.81✅ | 12.95/10.83⚠ |
| cbo_modified (hi, class) | 2.85/1.93⚠ | 2.75/1.79⚠ | 2.87/1.88⚠ | 2.37/2.35≈ |
| fan_in (hi, class) | 1.62/1.05⚠ | 1.49/0.95⚠ | 1.59/1.01⚠ | 1.26/1.32≈ |
| fan_out (hi, class) | 1.26/0.86⚠ | 1.15/0.82⚠ | 1.24/0.85⚠ | 1.04/0.98⚠ |
| nosi (hi, class) | 0.07/0.03⚠ | 0.06/0.02⚠ | 0.07/0.03⚠ | 0.05/0.04⚠ |
| tcc (lo, class) | 0.14/0.14≈ | 0.13/0.16⚠ | 0.13/0.15⚠ | 0.16/0.13✅ |
| lcc (lo, class) | 0.15/0.16⚠ | 0.15/0.19⚠ | 0.15/0.17⚠ | 0.18/0.15✅ |
| lcom_star (hi, class) | 0.29/0.28≈ | 0.28/0.32✅ | 0.28/0.29≈ | 0.30/0.28⚠ |
| loop_qty (hi, func) | 0.44/0.55✅ | 0.47/0.58✅ | 0.45/0.56✅ | 0.49/0.53✅ |
| comparisons_qty (hi, func) | 1.03/1.23✅ | 1.09/1.37✅ | 1.06/1.29✅ | 1.18/1.16≈ |
| variables_qty (hi, func) | 2.56/3.37✅ | 2.77/3.75✅ | 2.63/3.58✅ | 3.15/3.04≈ |
| unique_words_qty (hi, func) | 7.59/8.90✅ | 7.89/9.27✅ | 7.66/9.05✅ | 8.42/8.25≈ |
| math_ops_qty (hi, func) | 0.44/0.80✅ | 0.50/1.31✅ | 0.47/0.99✅ | 0.79/0.58⚠ |
