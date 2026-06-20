"""Production module B — same logic, commutative operands reshuffled (a Type-4 clone)."""


def tally(a, b, c):
    base = c + b * a
    scaled = tax(a) + base
    flagged = pen(b) + scaled
    if cap(c) == flagged:
        return flagged
    return base
