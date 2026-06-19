# Violations: tests that execute code but verify nothing, or assert only constants.

def test_runs_but_asserts_nothing():
    result = compute(2, 3)
    print(result)


def test_assert_true():
    do_something()
    assert True


def test_tautological_compare():
    do_something()
    assert 1 == 1


def test_assert_on_local_literal():
    x = 5
    assert x == 5


class TestService(unittest.TestCase):
    def test_no_assert(self):
        self.service.start()


# Non-violations: real assertions, delegated verification, stubs, and skips.

def test_real_assertion():
    assert add(2, 3) == 5


def test_value_from_call():
    x = compute()
    assert x == 5


def test_mixed_has_one_real():
    assert True
    assert add(2, 3) == 5


def test_raises_block():
    with pytest.raises(ValueError):
        parse("bad")


def test_delegated_helper():
    result = run()
    check_result(result)


def test_empty_stub():
    pass


@pytest.mark.skip
def test_skipped():
    do_something()


class TestUnit(unittest.TestCase):
    def test_real(self):
        self.assertEqual(add(2, 3), 5)


def helper_not_a_test():
    compute()
