# `self.fail(...)` is an assertion; an unrelated `.fail()` is not.
def test_fail_path():
    self.fail('boom')
    job.fail()  # unrelated .fail() — not an assertion
