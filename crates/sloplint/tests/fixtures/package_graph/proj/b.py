"""Module b: a relative import back to a sibling (intra-package edge)."""

from . import a  # relative import -> proj.a


def go():
    return a
