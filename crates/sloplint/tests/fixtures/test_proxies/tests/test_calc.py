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


def helper_not_a_test():
    # Not a `test_*` function, so its assert must not be counted.
    assert True
