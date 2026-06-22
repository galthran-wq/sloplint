# Per-function size/shape metrics (params, arity, ncss, exits, cyclomatic, cognitive,
# max_nesting, loc). Class methods and nested functions each get their own row.


# Baseline: two params, straight-line body.
def simple_add(a, b):
    return a + b


# No receiver -> arity equals params.
def free(a, b, c):
    return a


class C:
    # `self` is excluded from arity.
    def method(self, x, y):
        return x

    # @staticmethod: the first param is a real arg, so arity equals params.
    @staticmethod
    def stat(self, z):
        return z

    # `self` excluded; *args + **kwargs each count once.
    def variadic(self, *args, **kwargs):
        return args


# ncss counts statements, not lines: if + raise + aug-assign + return = 4 (blank line and the
# `def` header don't count); physical loc exceeds logical ncss.
def ncss_body(self, n):
    if n < 0:

        raise ValueError(n)
    self.total += n
    return self.total


# ncss is the function's own body: `def helper` (1) + `return` (1) = 2; helper's two statements
# belong to helper's own row.
def ncss_outer():
    def helper():
        a = 1
        return a

    return helper()


# exits count return/raise/yield in the own body; the nested function's return is excluded -> 3.
def exits_demo(x):
    def nested():
        return 1  # nested scope — not counted

    if x:
        raise ValueError
    yield x
    return x
