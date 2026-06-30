# SQM research dataset

Reusable dataset for any metric/threshold experiment. Built by `../build_dataset.py` from the 12k
stratified frame (`../frame.jsonl`) + the GraphQL labels (`../labels.jsonl`, `../contributors.jsonl`).
Three JSONL tables, all keyed by `full_name` → join freely. **Gitignored** (large, point-in-time
snapshot of live GitHub); regenerate with `python3 ../build_dataset.py`.

| file | rows | one row = | key fields |
| --- | ---: | --- | --- |
| `repos.jsonl` | ~12k | a repository | `full_name`, `sha`, `url`, `ok`; sample-meta (`stratum`, `stars`, `created_at`, `pushed_at`, `forks`, `size_kb`, `language`…); labels (`has_ci`, `merged_prs`, `releases`, `bugfix_ratio`, `contributors`, `recent_authors`, `commits_per_week`, `active_week_frac`, `ai_share`, `claude_share`…); the 118-feature aggregate panel flat as `m.*` / `tp.*` |
| `functions.jsonl` | ~10.5M | a function | `full_name`, `sha`, `file` (repo-relative), `function`, `loc`, `cyclomatic`, `cognitive`, `max_nesting`, `params`, `arity`, `ncss`, `exits`, `typed_params`, `has_docstring`, `has_return_annotation`, `file_loc`, `file_comment_density` |
| `classes.jsonl` | ~2.05M | a class | `full_name`, `sha`, `file`, `class`, `loc`, `methods` (NOM), `attributes` (NOA), `lcom4`, `wmc`, `dit`, `noc`, `cbo`, `is_abstract`, `has_docstring` |

Only `ok:true` rows in `repos.jsonl` have a measured panel + entries in the function/class tables.

## Why per-entity tables

The earlier `../features.jsonl` stored only the aggregate panel, which is blind to the per-function
/ per-class distribution. Benchmark-threshold methods (Alves/Ypma/Visser 2010; RTTOOL) operate on
the **entity distribution** — % of code volume over a threshold, LOC-weighted — so they need
`functions.jsonl` / `classes.jsonl`. `alves_thresholds.py` is the first consumer.

## Quick joins

```python
import json
repos = {json.loads(l)["full_name"]: json.loads(l) for l in open("repos.jsonl") if json.loads(l).get("ok")}
# per-function rows for engineered repos only:
eng = {fn for fn, r in repos.items() if r.get("has_ci") and (r.get("releases") or 0) > 0}
for l in open("functions.jsonl"):
    f = json.loads(l)
    if f["full_name"] in eng:
        ...  # f["cyclomatic"], f["loc"], ...
```
