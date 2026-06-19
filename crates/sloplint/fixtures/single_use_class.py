"""Fixture for the SLP111 single-use single-method class e2e test."""


class Formatter:
    def format(self, value):
        return f"<{value}>"


class Reused:
    def run(self, x):
        return x + 1


def main():
    formatter = Formatter()  # the only instantiation -> Formatter could be a function
    print(formatter.format(1))
    a = Reused()
    b = Reused()  # instantiated twice -> a legitimate, reused class
    return a, b
