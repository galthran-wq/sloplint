"""A concrete implementation that depends on both other packages (efferent)."""

from core.util import Helper
from iface.shapes import Shape


class Circle(Shape):
    def __init__(self, r: float) -> None:
        self.r = r
        self.helper = Helper()

    def area(self) -> float:
        return 3.14 * self.r * self.r
