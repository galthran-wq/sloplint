"""Fixture for the SLP130 docstring-drift e2e test."""


def parse_config(text):
    """Parse a config blob.

    Raises:
        ValueError: if the blob is invalid.
    """
    return text.splitlines()


def divide(a, b):
    """Divide a by b.

    Raises:
        ZeroDivisionError: if b is zero.
    """
    if b == 0:
        raise ZeroDivisionError("b is zero")
    return a / b
