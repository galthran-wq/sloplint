import pytest

from src.calc import add, mul


def test_add():
    assert add(1, 2) == 3
    assert add(0, 0) == 0


def test_mul():
    assert mul(2, 3) == 6


def test_raises():
    with pytest.raises(TypeError):
        add(1, "x")


def test_mul_table():
    # A branchy, substantive test (cognitive > 1) — not one-liner boilerplate. Exists so the
    # trivial-test rate over this suite is a real fraction, not 1.0 (#121).
    for a in range(3):
        for b in range(3):
            assert mul(a, b) == a * b


def helper_not_a_test():
    # Not a `test_*` function, so its assert must not be counted.
    assert True
