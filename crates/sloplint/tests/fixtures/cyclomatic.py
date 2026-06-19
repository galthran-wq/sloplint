"""Fixture for the cyclomatic-complexity metric e2e test.

Per-function cyclomatic complexity (CC = decisions + 1) under sloplint's documented
counting rules (if/elif, for, while, and/or, except, case; ternary and comprehension
`if`/`for` count):

  trivial        -> CC 1   (low)
  comprehension  -> CC 3   (low):   for + if
  moderate       -> CC 12  (moderate, 11-20):
                    if + and + and        = 3
                    if + or  + or         = 3
                    for                   = 1
                    if + and              = 2
                    while + or            = 2
                    => 11 decisions + 1   = 12
"""


def trivial():
    return 1


def comprehension(xs):
    return [x for x in xs if x > 0]


@memoize
def moderate(a, b, c, d, e):
    if a and b and c:
        return 1
    if d or e or a:
        return 2
    for x in range(a):
        if x and b:
            return x
    while b or c:
        b -= 1
    return 0
