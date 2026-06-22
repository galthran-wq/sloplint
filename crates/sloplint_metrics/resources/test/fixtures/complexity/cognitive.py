# Cognitive-complexity fixtures (SonarSource-style). One function per case; the snapshot pins
# each function's cognitive score. Nesting is penalized; flat structures are read linearly.


# A flat `match` is one structure counted once regardless of case count -> 1.
def classify(x):
    match x:
        case 1:
            return "one"
        case 2:
            return "two"
        case 3:
            return "three"
        case _:
            return "other"


# Nested ifs are penalized: if(1+0) + if(1+1) + if(1+2) = 6.
def tangle(a, b, c):
    if a:
        if b:
            if c:
                return 1
    return 0


# `a and b and c` is one And sequence (+1); `a and b or c` is two (+2): if(1)+1 + if(1)+2 = 5.
def boolean_ops(a, b, c):
    if a and b and c:
        return 1
    if a and b or c:
        return 2
    return 0


# A top-level ternary: +1.
def ternary_top(a, b):
    x = a if b else 0
    return x


# Ternary nested inside an `if` body: if(1) + ternary(1+1) = 3.
def ternary_in_if(a, b, c):
    if a:
        return b if c else 0
    return 0


# A comprehension `if` filter, top level: +1.
def comp_filter_top(xs):
    return [x for x in xs if x > 0]


# Comprehension filter nested in a loop: for(1) + filter(1+1) = 3.
def comp_filter_in_loop(xss):
    out = []
    for xs in xss:
        out.append([x for x in xs if x])
    return out


# `with` adds no increment and no nesting, so the inner `if` stays at level 0 -> 1.
def with_if(path):
    with open(path) as fh:
        if fh:
            return 1
    return 0


# `try` adds nothing; only the `except` handler increments -> 1.
def try_except(x):
    try:
        return risky(x)
    except ValueError:
        return 0


# if(1+0) + else(+1 flat) = 2.
def if_else(a):
    if a:
        return 1
    else:
        return 0


# match(+1) + the guard's `a and b` boolean op (+1) = 2 (regression: guards were dropped).
def match_guard(x, a, b):
    match x:
        case 1 if a and b:
            return 1
        case _:
            return 0


# `with` is not a flow break, but the `a and b` in its context expression counts -> 1.
def with_item_cond(a, b):
    with make(a and b) as fh:
        return fh


# Documented simplification: both ternaries score at statement nesting (0): 1 + 1 = 2.
def nested_ternary(a, b, c, d, e):
    return a if b else (c if d else e)
