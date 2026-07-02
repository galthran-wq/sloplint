# Alves profile vs many labels — does % code in high-risk bands discriminate?

Per-repo % of code in high+very-high bands. 'hi' = top tercile of the label (or =1 for binary), 'lo' = bottom. A discriminating label → lower % for the 'good' side.


## ALL repos (raw) — N=11459

| label | cognitive: hi/lo | cyclomatic: hi/lo | func LOC: hi/lo |
|---|---|---|---|
| stars (hi=more) | 18/9↑ | 17/7↑ | 19/3↑ |
| contributors | 16/12↑ | 16/10↑ | 18/6↑ |
| team size | 15/12↑ | 15/10↑ | 18/6↑ |
| age | 12/20↓ | 10/20↓ | 11/20↓ |
| bugfix rate | 18/8↑ | 18/5↑ | 20/0↑ |
| AI share | 14/13↑ | 14/11↑ | 15/12↑ |
| has CI | 14/17↓ | 13/15↓ | 14/17↓ |
| engineered | 15/14↑ | 15/14↑ | 18/13↑ |

_(cell = high-group% / low-group%; ↓ = 'good' side has LESS complex code (discriminates), ↑ = more, ≈ = flat)_

## size-controlled (8k–30k LOC) — N=2656

| label | cognitive: hi/lo | cyclomatic: hi/lo | func LOC: hi/lo |
|---|---|---|---|
| stars (hi=more) | 18/17≈ | 17/18≈ | 20/19↑ |
| contributors | 15/20↓ | 14/20↓ | 17/20↓ |
| team size | 15/19↓ | 14/19↓ | 18/19↓ |
| age | 14/21↓ | 13/22↓ | 17/23↓ |
| bugfix rate | 18/17↑ | 18/16↑ | 20/19↑ |
| AI share | 18/16↑ | 19/15↑ | 19/18↑ |
| has CI | 16/20↓ | 16/20↓ | 18/21↓ |
| engineered | 15/20↓ | 15/20↓ | 17/20↓ |

_(cell = high-group% / low-group%; ↓ = 'good' side has LESS complex code (discriminates), ↑ = more, ≈ = flat)_
