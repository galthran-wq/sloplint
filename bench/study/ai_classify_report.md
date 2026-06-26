# Can logreg on the 7 axes separate AI-heavy vs non-AI?

N=3527 (heavy=339, non=3188); 5-fold CV AUROC. The 7 axes are size-residualized.

| feature set | CV AUROC |
|---|---:|
| A. 7 size-residualized axes | **0.741** |
| B. log(LOC) only (size baseline) | **0.694** |
| C. 7 axes + log(LOC) | **0.793** |

## Model A coefficients (which axis drives AI-separation, size removed)

| axis | std weight |
|---|---:|
| F5 docs/typing | +0.87 |
| F1 avg-complexity | +0.63 |
| F6 module-struct | +0.27 |
| F2 tail | -0.22 |
| F3 architecture | -0.14 |
| F4 test-substance | -0.04 |
| F7 comments/toplevel | +0.00 |

**Read:** size alone (B) gives AUROC 0.69; the size-controlled construction axes (A) give 0.74. Adding size to the axes (C) -> 0.79. The axes add real signal.
