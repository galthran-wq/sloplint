# Violations: trivial bodies topped with a comment admitting they're unfinished.

def handle(event):
    # not sure how to wire this up yet
    raise NotImplementedError


# TODO: implement this properly later
def parse(source):
    pass


def render(node):
    # placeholder - fill in the real logic
    ...


def total(rows):
    # come back to this once the schema settles
    return None


def connect():
    # figure this out
    print("not connected")


# Non-violations: finished code, plain tags over real bodies, abstract stubs, docstrings.

def add(a, b):
    return a + b


def refactor_me(a, b):
    # TODO: refactor this for clarity
    return a + b


def quietly_unsure(xs):
    # not sure this covers every edge case
    return [x * 2 for x in xs]


def silent_stub():
    raise NotImplementedError


class Shape:
    @abstractmethod
    def area(self):
        # implement this in subclasses
        ...


def documented(self):
    """Subclasses must implement this method."""
    raise NotImplementedError


class Reader(Protocol):
    def read(self, key):
        # implement this in concrete readers
        ...


config = {}  # fill in the defaults later
def load_config():
    return config
