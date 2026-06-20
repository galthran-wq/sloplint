"""A submodule that imports back up into the parent package (a cross-package edge)."""

from ..b import go  # relative import up one level -> proj.b


def assist():
    return go()
