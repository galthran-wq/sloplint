# count_assertions descends into a local helper, so asserting through it is not assertion-free.
def test_uses_local_helper():
    def check(v):
        assert v > 0

    check(f(3))
