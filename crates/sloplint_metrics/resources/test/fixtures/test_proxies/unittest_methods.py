# The assertion-free check runs over test* methods of a unittest class too.
class TestThing:
    def test_asserts(self):
        self.assertEqual(f(1), 1)

    def test_theater(self):
        result = f(2)
        print(result)
