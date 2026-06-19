"""Fixture for the Type-3 gapped-clone e2e test.

`alpha` and `beta` are the same computation with statements reordered (the `seen`/`total`
setup swapped, and swapped again inside the loop). The ordered shingle pass misses this;
the statement-bag pass (enabled with `[clone] detect_gapped = true`) catches it.
"""


def alpha(rows):
    total = 0
    seen = set()
    for row in rows:
        total += row.value
        seen.add(row.id)
    ratio = total / len(seen)
    return ratio


def beta(rows):
    seen = set()
    total = 0
    for row in rows:
        seen.add(row.id)
        total += row.value
    ratio = total / len(seen)
    return ratio
