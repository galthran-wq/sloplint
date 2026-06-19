"""Fixture for the SLP110 pass-through-wrapper e2e test."""


def fetch(user_id, db):
    return _backend.fetch(user_id, db)


def fetch_checked(user_id, db):
    if user_id < 0:
        raise ValueError("bad id")
    return _backend.fetch(user_id, db)


def add(a, b):
    return a + b
