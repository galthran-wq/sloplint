"""Fixture for the clone-coverage metric e2e test.

Three functions: `total_price` and `sum_costs` are structural clones (same shape, renamed
identifiers — a Type-2 clone the engine detects at the default 0.85 similarity), while
`parse_config` is unrelated. So at default `[clone]` settings:

  clone-involved functions = 2 of 3  -> 66.7% function coverage, 1 clone pair.
"""


def total_price(items):
    total = 0
    for item in items:
        total += item.price * item.quantity
    return total


def sum_costs(products):
    acc = 0
    for product in products:
        acc += product.price * product.quantity
    return acc


def parse_config(path):
    with open(path) as handle:
        data = handle.read()
    return data.strip().splitlines()
