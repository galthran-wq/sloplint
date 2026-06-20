def documented(x):
    """Return x doubled.

    A two-paragraph docstring on a public function.
    """
    return x * 2


def undocumented(x):
    return x + 1


def _private_helper(x):
    return x - 1


class Service:
    """A documented public class."""

    def run(self):
        return 1
