"""One abstract + one concrete class in the same module, for a fractional abstractness (A=0.5)."""

from abc import ABC, abstractmethod


class Source(ABC):
    @abstractmethod
    def read(self) -> bytes: ...


class FileSource:
    def __init__(self, path: str) -> None:
        self.path = path

    def read(self) -> bytes:
        return b""
