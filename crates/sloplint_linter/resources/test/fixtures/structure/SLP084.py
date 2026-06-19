# Violations: data-structure literals nested past the default depth (3).

config = {"a": [{"b": [1]}]}

matrix = [[[[1, 2], [3, 4]]]]

routes = {
    "api": {"v1": {"users": [{"id": 1}]}},
}

cells = {key: [[[value]] for value in row] for key, row in data}


# Non-violations: shallow data, and deep *control flow* over shallow data.

flat = {"a": 1, "b": 2}

single_level = {"a": [1, 2, 3]}

two_levels = {"a": {"b": 1}}

pairs = [(1, "a"), (2, "b")]


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
