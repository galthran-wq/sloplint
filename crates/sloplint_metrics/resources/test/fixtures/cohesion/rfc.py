# RFC (Response For a Class) fixture: |M ∪ R| — own methods plus the distinct methods they
# invoke, keyed by trailing callee name. The snapshot pins the response-set sizes.


# M = {area, describe}; describe calls self.area(), which folds back into M (no growth). rfc = 2.
class Shape:
    def area(self):
        return 0

    def describe(self, verbose):
        if verbose and self.area() > 0:
            return "big"
        return "small"


# M = {parse, render}; R adds the two remote calls run (self.parser.run) and go
# (self.formatter.go). rfc = |{parse, render, run, go}| = 4.
class Facade:
    def parse(self, t):
        return self.parser.run(t)

    def render(self, n):
        return self.formatter.go(n)


# Free-function / builtin calls count as invocations too (Python has no method/function
# distinction): M = {run}, R adds range and len. rfc = |{run, range, len}| = 3.
class Loop:
    def run(self, xs):
        for _ in range(len(xs)):
            pass


# A call to a nested helper counts (inner is invoked in response), but the helper is not itself an
# own method: M = {m}, R adds inner. rfc = |{m, inner}| = 2.
class Nested:
    def m(self, flag):
        def inner(xs):
            return xs

        if flag:
            return inner([])
        return None


# A parametrized decorator (route) and a default-arg call (make_default) run at definition time,
# not in response to a message, so neither counts. M = {handle}, R adds only the body call fetch.
# rfc = |{handle, fetch}| = 2.
class Endpoint:
    @app.route("/x")
    def handle(self, data=make_default()):
        return self.client.fetch(data)


# No methods -> empty response set. rfc = 0.
class Empty:
    pass
