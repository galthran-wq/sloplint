"""Module a: exercises absolute, from-submodule, and TYPE_CHECKING imports."""

from typing import TYPE_CHECKING

import os  # stdlib — not first-party, no edge
import proj.b  # absolute import of a sibling submodule
from proj.sub import helper  # `helper` is a submodule -> edge to proj.sub.helper

if TYPE_CHECKING:
    from proj.c import Thing  # type-checking only -> marked, still an edge to proj.c


def run(thing: "Thing") -> None:
    proj.b.go()
    helper.assist()
    print(os.getcwd())
