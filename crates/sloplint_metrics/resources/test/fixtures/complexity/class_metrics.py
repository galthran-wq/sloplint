# Per-class metrics (methods/attributes/lcom4/wmc/loc). Single-file, so the cross-file
# dit/noc/cbo are 0 here. The snapshot pins the size/cohesion/weight figures.


# All methods share self.total -> cohesive (lcom4 1); attributes = total, name.
class Counter:
    def __init__(self):
        self.total = 0
        self.name = 'c'

    def add(self, n):
        self.total += n

    def show(self):
        return self.total


# parse/render touch disjoint attributes -> lcom4 2.
class Utils:
    def parse(self, t):
        return self.parser.run(t)

    def render(self, n):
        return self.formatter.go(n)


# WMC sums method cyclomatic: calc CC 1 + check (if + and over base 1 = 3) = 4.
class WmcDemo:
    def calc(self):
        return 1

    def check(self, x):
        if x and x > 0:
            return True
        return False


# No methods -> no weight.
class Empty:
    pass


# A method's WMC is its own-body cyclomatic: only m's `if` counts (CC 2); the nested `inner`'s
# for+if belong to inner, not to m.
class WmcNested:
    def m(self, flag):
        def inner(xs):
            for x in xs:
                if x:
                    return x

        if flag:
            return inner([])
        return None
