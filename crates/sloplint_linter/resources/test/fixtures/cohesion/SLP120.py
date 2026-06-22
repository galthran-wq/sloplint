"""Fixture for SLP120 low-cohesion 'god class' detection (violations + non-violations)."""

import dataclasses

import attrs
from typing import Protocol


# --- violation: a catch-all class bundling two unrelated concerns ---


class Utils:
    def parse(self, text):
        return self.parser.run(text)

    def tokenize(self, text):
        return self.parser.split(text)

    def render(self, node):
        return self.formatter.render(node)

    def export(self, node):
        return self.formatter.write(node)


# --- violation: a bare user `@frozen` is not an allowlist (only @dataclass/attr[s] are) ---


@frozen
class FrozenUtils:
    def parse(self, text):
        return self.parser.run(text)

    def tokenize(self, text):
        return self.parser.split(text)

    def render(self, node):
        return self.formatter.render(node)


# --- non-violation: two unrelated methods, but under the lcom4_min_methods floor ---


class Pair:
    def left(self):
        return self.a

    def right(self):
        return self.b


# --- non-violation: @attrs.define marks a data container, not a god class ---


@attrs.define
class DataBag:
    def parse(self, text):
        return self.parser.run(text)

    def tokenize(self, text):
        return self.parser.split(text)

    def render(self, node):
        return self.formatter.render(node)


# --- non-violation: cohesive class, all methods revolve around shared state ---


class Counter:
    def __init__(self):
        self.total = 0

    def add(self, n):
        self.total += n

    def double(self):
        self.add(self.total)

    def value(self):
        return self.total


# --- non-violation: a dataclass is a data container, not a god class ---


@dataclasses.dataclass
class Config:
    def host(self):
        return self.h

    def port(self):
        return self.p

    def scheme(self):
        return self.s


# --- non-violation: a Protocol is an interface ---


class Store(Protocol):
    def get(self, key):
        return self.a

    def put(self, key):
        return self.b

    def drop(self, key):
        return self.c
