"""Fixture for SLP130 docstring drift (Raises:/Returns: vs the body)."""


# --- violations ---


def load(path):
    """Read and parse the file.

    Raises:
        ValueError: if the file is malformed.
    """
    return open(path).read()


def configure(options) -> None:
    """Apply the options.

    Returns:
        The applied configuration.
    """
    print(options)


def fetch(url):
    """Fetch a resource (NumPy style).

    Returns
    -------
    bytes
        The body.
    """
    log(url)


def validate(value):
    """Validate a value (NumPy Raises).

    Raises
    ------
    ValueError
        If the value is out of range.
    """
    log(value)


def connect(dsn):
    """Open a connection (Sphinx style).

    :raises ConnectionError: if the host is unreachable.
    """
    log(dsn)


# --- non-violations: the docstring matches the body ---


def divide(a, b):
    """Divide two numbers.

    Returns:
        The quotient.

    Raises:
        ZeroDivisionError: if b is zero.

        Note: integer and float division both supported.
    """
    if b == 0:
        raise ZeroDivisionError("b is zero")
    return a / b


def stream(items):
    """Yield items one by one.

    Returns:
        Each item in turn.
    """
    for item in items:
        yield item


def summarize(text):
    """Return a short summary with no special sections."""
    return text[:80]
