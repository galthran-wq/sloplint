"""Fixture for SLP150 comment/blank padding detection (violations + non-violations)."""


# --- violation: a function that is mostly narration and spacing ---


def accumulate(rows):
    # start the running total at zero
    total = 0

    # iterate over each row in the input
    for row in rows:
        # pull the value out of the row
        value = row.value

        # add it onto the total
        total += value

    # finally, return the total we built up
    return total


# --- non-violation: dense real code, no padding ---


def normalize(values):
    total = sum(values)
    if total == 0:
        return values
    scaled = [v / total for v in values]
    rounded = [round(x, 3) for x in scaled]
    return [min(x, 1.0) for x in rounded]
