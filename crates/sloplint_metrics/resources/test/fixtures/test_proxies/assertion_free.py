import pytest


def test_good_simple():
    # Clean unit test — cognitive 0, but it asserts. Must NOT be flagged.
    assert f(2) == 4


def test_good_raises():
    with pytest.raises(ValueError):
        f(-1)


def test_theater_print():
    # 'Test theater': loops and prints, asserts nothing. MUST be flagged.
    for x in (0, 1, 2):
        print(f(x))


def test_theater_stub():
    pass
