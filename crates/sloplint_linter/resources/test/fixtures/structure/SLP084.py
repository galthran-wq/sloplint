# Violations: data-structure literals nested past the default depth (3).

config = {"a": [{"b": [1]}]}

matrix = [[[[1, 2], [3, 4]]]]

routes = {
    "api": {"v1": {"users": [{"id": 1}]}},
}

cells = {key: [[[value]] for value in row] for key, row in data}

# A call breaks the container chain: the inner 4-deep literal is its own root.
wrapped = [wrap([[[[1]]]])]

# A deep literal in a comprehension's iterable / condition is a fresh root too.
from_iter = [process(x) for x in [[[[1]]]]]
from_cond = [x for x in items if x in [[[[1]]]]]


# Non-violations: shallow data, and deep *control flow* over shallow data.

flat = {"a": 1, "b": 2}

single_level = {"a": [1, 2, 3]}

two_levels = {"a": {"b": 1}}

pairs = [(1, "a"), (2, "b")]

# Assignment-target unpacking is a Store-context destructuring, not a data literal.
[[[[a]]]] = source


def deeply_nested_control_flow(items):
    for item in items:
        if item:
            while item.next:
                with item.lock:
                    try:
                        value = [item.value]
                    except KeyError:
                        value = []
    return value
