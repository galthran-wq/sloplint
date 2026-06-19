"""Fixture for the SLP003 comment-deodorant e2e test."""


def classify(rows, mode):
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


def clean(rows):
    out = []
    for row in rows:
        if row.active:
            for cell in row.cells:
                if cell.value:
                    out.append(cell)
    return out
