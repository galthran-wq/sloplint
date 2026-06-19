"""A realistic-looking test module exercising SLP070 end to end.

It mixes genuine tests with the worthless shapes SLP070 targets, so the CLI test can
assert exactly which functions are flagged when run over real Python.
"""

import unittest

import pytest

from mypkg.calc import add, divide
from mypkg.parser import parse


# --- worthless tests (should be flagged) -------------------------------------

def test_divide_runs():
    # Calls production code but never checks the result — coverage without verification.
    result = divide(10, 2)
    print(result)


def test_addition_is_addition():
    add(2, 3)
    assert True


def test_constants_are_equal():
    assert 1 == 1


def test_echo_local():
    expected = 42
    assert expected == 42


# --- genuine tests (must not be flagged) -------------------------------------

def test_add_returns_sum():
    assert add(2, 3) == 5


@pytest.mark.parametrize("a, b, expected", [(1, 1, 2), (2, 2, 4)])
def test_add_parametrized(a, b, expected):
    assert add(a, b) == expected


def test_divide_by_zero_raises():
    with pytest.raises(ZeroDivisionError):
        divide(1, 0)


def test_parse_via_helper():
    parsed = parse("a=1")
    check_parsed(parsed)


@pytest.mark.skip(reason="not implemented yet")
def test_future_feature():
    do_the_thing()


def check_parsed(parsed):
    assert parsed["a"] == 1


class TestCalculator(unittest.TestCase):
    def test_add(self):
        self.assertEqual(add(2, 3), 5)

    def test_divide_smoke(self):
        # Smoke test that asserts nothing — flagged.
        divide(10, 2)
