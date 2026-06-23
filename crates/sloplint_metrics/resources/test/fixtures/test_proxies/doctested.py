"""A production module tested primarily via doctests.

Its examples live here, not in a separate test file, so the path-based test:code
ratio is blind to them — that is the blind spot the doctest signals fix.

    >>> add(2, 3)
    5
"""


def add(a, b):
    """Sum two numbers.

    >>> add(2, 3)
    5
    >>> add(-1, 1)
    0
    """
    return a + b


def greet(name):
    """Return a greeting.

    >>> greet("Sam")
    'hello Sam'
    """
    return f"hello {name}"


def undocumented(x):
    return x * 2


def documented_no_doctest(x):
    """Double x (no example here)."""
    return x * 2
