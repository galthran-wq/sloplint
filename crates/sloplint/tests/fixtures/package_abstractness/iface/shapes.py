"""Pure interfaces: an ABC and a Protocol, both abstract under the #70 heuristic."""

from abc import ABC, abstractmethod
from typing import Protocol


class Shape(ABC):
    @abstractmethod
    def area(self) -> float: ...


class Renderer(Protocol):
    def render(self) -> None: ...
