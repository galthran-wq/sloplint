"""Fixture for SLP110 pass-through wrapper detection (violations + non-violations)."""


# --- violations: pure pass-through wrappers ---


def fetch_user(user_id, db):
    return _impl.fetch_user(user_id, db)


def render(template, context):
    """Forwarding wrapper, docstring and all."""
    return engine.render(template, context)


async def save(record):
    return await store.save(record)


class Service:
    def handle(self, request, timeout):
        return self._client.handle(request, timeout)


def relay(*args, **kwargs):
    dispatch(*args, **kwargs)


# --- non-violations: the wrapper does real work, or there is no pass-through ---


def fetch_validated(user_id, db):
    if user_id < 0:
        raise ValueError("bad id")
    return _impl.fetch_user(user_id, db)


def normalize(value):
    return _impl.process(value.strip())


def with_default(a):
    return _impl.run(a, retries=3)


def add(a, b):
    return a + b


def make_client():
    return Client()


class Base:
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
