"""Fixture for the cognitive-complexity metric e2e test.

Per-function SonarSource cognitive complexity:
  classify -> 1   (a flat `match` is one structure, read linearly)
  tangle   -> 6   (if + nested if + nested if: 1 + 2 + 3, nesting is penalized)
"""


def classify(value):
    match value:
        case 1:
            return "one"
        case 2:
            return "two"
        case _:
            return "other"


def tangle(a, b, c):
    if a:
        if b:
            if c:
                return 1
    return 0
