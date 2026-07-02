# Alves/SIG thresholds by reference cohort

Repos: all=574, engineered=245, non-engineered=329. Threshold = metric value at the q-quantile of LOC-weighted code volume.

## cyclomatic — LOC-weighted thresholds (70 / 80 / 90 %)


| reference            | 70% | 80% | 90% |
| -------------------- | --- | --- | --- |
| all (typical GitHub) | 9   | 12  | 20  |
| engineered           | 8   | 12  | 20  |
| non-engineered       | 9   | 13  | 21  |


### cyclomatic by repo size (all repos)


| LOC bucket | n   | 70% | 80% | 90% |
| ---------- | --- | --- | --- | --- |
| 50–2000    | 225 | 8   | 11  | 17  |
| 2000–8000  | 128 | 9   | 13  | 22  |
| 8000–30000 | 138 | 9   | 13  | 22  |
| 30000–∞    | 83  | 10  | 15  | 25  |


### cyclomatic weighted vs unweighted (all) — why Alves weights by LOC


|              | 70% | 80% | 90% |
| ------------ | --- | --- | --- |
| LOC-weighted | 9   | 12  | 20  |
| unweighted   | 4   | 5   | 9   |


## cognitive — LOC-weighted thresholds (70 / 80 / 90 %)


| reference            | 70% | 80% | 90% |
| -------------------- | --- | --- | --- |
| all (typical GitHub) | 11  | 17  | 33  |
| engineered           | 10  | 16  | 31  |
| non-engineered       | 11  | 18  | 34  |


### cognitive by repo size (all repos)


| LOC bucket | n   | 70% | 80% | 90% |
| ---------- | --- | --- | --- | --- |
| 50–2000    | 225 | 10  | 15  | 29  |
| 2000–8000  | 128 | 11  | 18  | 34  |
| 8000–30000 | 138 | 11  | 18  | 34  |
| 30000–∞    | 83  | 13  | 20  | 39  |


### cognitive weighted vs unweighted (all) — why Alves weights by LOC


|              | 70% | 80% | 90% |
| ------------ | --- | --- | --- |
| LOC-weighted | 11  | 17  | 33  |
| unweighted   | 3   | 5   | 10  |


