import pytest


def helper():
    assert True  # not a test function — not counted


def test_one():
    assert 1 == 1
    assert 2 == 2


def test_two():
    with pytest.raises(ValueError):
        do()


class TestThing:
    def test_method(self):
        self.assertEqual(1, 1)
        self.assertTrue(True)

    def not_a_test(self):
        assert False  # not test_* — not counted
