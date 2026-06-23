# Positive: literal-dispatch ladder — 4 `==` branches on the same subject `kind`.
def handle(kind):
    if kind == "a":
        return 1
    elif kind == "b":
        return 2
    elif kind == "c":
        return 3
    elif kind == "d":
        return 4


# Positive: isinstance ladder — 4 type checks on the same subject `node`.
def visit(node):
    if isinstance(node, int):
        return "int"
    elif isinstance(node, str):
        return "str"
    elif isinstance(node, list):
        return "list"
    elif isinstance(node, dict):
        return "dict"


# Negative: short ladder — only 3 branches, at the default limit (not past it).
def small(kind):
    if kind == "a":
        return 1
    elif kind == "b":
        return 2
    elif kind == "c":
        return 3


# Negative: heterogeneous conditions — not a uniform dispatch.
def mixed(x, y):
    if x == "a":
        return 1
    elif y > 3:
        return 2
    elif x == "c":
        return 3
    elif helper(x):
        return 4


# Negative: 4 `==` branches but on different subjects — not a single-value dispatch.
def different_subjects(a, b, c, d):
    if a == 1:
        return 1
    elif b == 2:
        return 2
    elif c == 3:
        return 3
    elif d == 4:
        return 4
