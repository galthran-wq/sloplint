"""Root of a first-party inheritance chain that spans two modules."""


class Shape:
    """A first-party root: its only base is the implicit `object` (external → invisible)."""

    def area(self):
        return 0

    def describe(self, verbose):
        # WMC contribution: `if` (+1) plus the `and` chain (+1) over the base 1 = 3.
        if verbose and self.area() > 0:
            return "a shape with area"
        return "a shape"
