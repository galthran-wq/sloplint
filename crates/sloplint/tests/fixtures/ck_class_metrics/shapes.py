"""Derived classes — DIT must follow the `Shape` base across the module boundary."""

from base import Shape
from third_party import Widget


class Circle(Shape):
    """DIT 1: one first-party hop up to Shape."""

    def __init__(self, r):
        self.r = r

    def area(self):
        return 3 * self.r * self.r


class Unit(Circle):
    """DIT 2: Unit -> Circle -> Shape."""

    def area(self):
        # A branchy override: `for` (+1) + `if` (+1) over base 1 = WMC 3.
        for _ in range(1):
            if self.r:
                return 1
        return 0


class Panel(Widget):
    """DIT 0: Widget is third-party, so its depth is invisible."""

    def render(self):
        return None
