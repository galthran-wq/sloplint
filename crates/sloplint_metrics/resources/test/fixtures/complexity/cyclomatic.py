# Cyclomatic complexity + max nesting fixtures. One function per case (nested functions are
# their own rows); the snapshot pins cyclomatic/cognitive/max_nesting per function.


# for + if + and => cyclomatic 1+3=4, nesting 2.
def branchy(xs):
    total = 0
    for x in xs:
        if x and x > 0:
            total += x
    return total


# Regression: branch keywords inside a string literal must NOT be counted -> cyclomatic 1.
def keywords_in_strings():
    msg = "if and or while for except"
    return msg


# Nested function logic is not double-counted: `inner` owns its `if`; `outer` owns only the
# comprehension `for`, and outer's cognitive must not absorb inner's branch.
def outer(xs):
    def inner(x):
        if x:
            return 1
        return 0

    return [inner(x) for x in xs]
