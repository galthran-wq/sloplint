# Per-function syntactic *Qty counters. Each function's comment states the expected non-zero
# counts; the snapshot pins all seven. Nested defs/lambdas are excluded (own-body), like ncss.


def loops(xs):
    # loops: for(1) + while(1) + comprehension 2 generators(2) = 4. numbers: the `1` in `x -= 1`.
    # variables: {x, row, a} = 3 (xs is a parameter). unique_words: {xs, x, a, row} = 4.
    for x in xs:
        while x:
            x -= 1
    return [a for row in xs for a in row]


def comparisons(a, b):
    # comparisons: (a == b) 1 + (a < b < 10) 2 + (a is None) 1 = 4. `or` is not a comparison.
    # numbers: {10} = 1. unique_words: {a, b} = 2 (None is a keyword literal, not an identifier).
    return (a == b) or (a < b < 10) or (a is None)


def literals():
    # numbers: 1, 2, 3.5 = 3 (the `n` in the f-string is a name). strings: "x" + f"y{n}" = 2.
    # math_ops: 1 + 2 = 1. variables: {n, s, t} = 3.
    n = 1 + 2
    s = "x"
    t = f"y{n}"
    return 3.5


def math_ops(a, b):
    # math_ops: +, -, *, %, << = 5 (bit ops count; augmented assignment / unary would not).
    # numbers: {2, 1} = 2.
    return a + b - a * b % 2 << 1


def variables(flag):
    # variables: {a, b, c} = 3 (c is reassigned, counted once; flag is a parameter, excluded).
    # numbers: 1, 2, 3, 4 = 4. math_ops: a + b + c = 2.
    a = 1
    b, c = 2, 3
    c = 4
    return a + b + c


def words(alpha, beta):
    # unique_words: {alpha, beta, gamma, print} = 4. variables: {gamma} = 1.
    gamma = alpha
    print(beta, gamma, alpha)
    return gamma
