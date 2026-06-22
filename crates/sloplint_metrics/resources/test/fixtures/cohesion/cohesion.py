# Fixtures for LCOM4 class cohesion (one class per case; snapshot pins methods/components/attributes).
# Constructors (__init__/__new__/__post_init__) are excluded from the method graph.


# Cohesive: both methods use self.total, and one calls the other -> a single component.
class Counter:
    def __init__(self):
        self.total = 0

    def add(self, n):
        self.total += n

    def reset_and_add(self, n):
        self.total = 0
        self.add(n)


# Two disjoint concepts: {parse, tokenize} share self.parser; {render, format} share
# self.formatter -> two components.
class Utils:
    def parse(self, text):
        return self.parser.run(text)

    def tokenize(self, text):
        return self.parser.split(text)

    def render(self, node):
        return self.formatter.render(node)

    def format(self, node):
        return self.formatter.pretty(node)


# No shared attribute, but `a` calls `b` -> linked into one component.
class MethodCalls:
    def a(self, x):
        return self.b(x)

    def b(self, x):
        return x + 1


# __init__ touches both attrs but must NOT connect the two disjoint methods -> two components,
# and __init__ is not counted among the methods.
class ConstructorExcluded:
    def __init__(self):
        self.a = 1
        self.b = 2

    def use_a(self):
        return self.a

    def use_b(self):
        return self.b


# Two static helpers, no shared instance state -> two components.
class Statics:
    @staticmethod
    def one():
        return 1

    @staticmethod
    def two():
        return 2


# A single method is trivially one component.
class Single:
    def only(self):
        return 1


# Regression: `helper`/`lambda` re-bind `self`, so their `self.shared` must NOT be credited to
# `m1`. Correct LCOM4 = 2 ({m1}, {m2, m3}).
class ShadowedReceiver:
    def m1(self):
        def helper(self):
            return self.shared

        f = lambda self: self.shared
        return helper, f

    def m2(self):
        return self.shared

    def m3(self):
        return self.shared


# register/count share cls.registry; touch shares the `registry` name -> all one component.
class ClassmethodReceiver:
    @classmethod
    def register(cls, x):
        cls.registry.append(x)

    @classmethod
    def count(cls):
        return len(cls.registry)

    def touch(self):
        return self.registry


# Two otherwise-disjoint methods that both call self.__init__() must stay disjoint -> two
# components (the excluded constructor never links them).
class ResetViaInit:
    def __init__(self):
        self.a = 0
        self.b = 0

    def reset_a(self):
        self.__init__()
        return self.a

    def reset_b(self):
        self.__init__()
        return self.b


# self.value is referenced inside a comprehension/call — full traversal must see it -> one
# component.
class NestedAccess:
    def a(self):
        return [v for v in self.value if v]

    def b(self):
        return sum(self.value)
