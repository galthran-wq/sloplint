"""Fixture for SLP003 comment-deodorant smell (violations + non-violations)."""


# --- violation: hard AND heavily commented (the composite smell) ---


def process(rows, mode):
    result = []                      # output accumulator
    for row in rows:                 # walk every row
        if row.active:               # only the active ones
            for cell in row.cells:   # each cell in the row
                if cell.value:       # that actually has a value
                    if mode == "a":  # mode A keeps the cell
                        result.append(cell)
                    elif mode == "b":  # mode B leaves a hole
                        result.append(None)
    return result


# --- non-violation: equally complex, but the code carries its own weight ---


def tidy(rows):
    out = []
    for row in rows:
        if row.active:
            for cell in row.cells:
                if cell.value:
                    out.append(cell)
    return out


# --- non-violation: heavily commented, but trivial (no real complexity) ---


def greet(name):
    # build a friendly greeting
    # using the provided name
    # then hand it back to the caller
    return f"hello {name}"
