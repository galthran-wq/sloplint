"""Production module A."""


def score(a, b, c):
    base = a * b + c
    scaled = base + tax(a)
    flagged = scaled + pen(b)
    if flagged == cap(c):
        return flagged
    return base
