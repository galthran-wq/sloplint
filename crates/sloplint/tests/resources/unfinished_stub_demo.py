"""A realistic module mixing finished code with unfinished-stub slop.

Used by the CLI end-to-end test to confirm SLP034 fires only on the trivial
bodies that also carry a comment admitting they're unfinished.
"""

from abc import abstractmethod


# --- unfinished stubs (should be flagged) ------------------------------------

def export_report(report):
    # TODO: implement this once the format is decided
    raise NotImplementedError


def normalize(record):
    # not sure how to handle the legacy rows here
    ...


# come back to this when the cache layer lands
def warm_cache(keys):
    pass


# --- finished / legitimate code (must not be flagged) ------------------------

def total(rows):
    return sum(row.amount for row in rows)


def slugify(name):
    # TODO: handle unicode better someday
    return name.strip().lower().replace(" ", "-")


class Storage:
    @abstractmethod
    def read(self, key):
        # subclasses implement this
        ...

    def write(self, key, value):
        self._data[key] = value


def documented_interface(self):
    """Subclasses must implement this hook."""
    raise NotImplementedError
