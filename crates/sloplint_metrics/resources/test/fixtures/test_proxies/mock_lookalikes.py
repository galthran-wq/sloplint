# Only real unittest assertions count: mock configuration and user helpers do not.
def test_mock_calls():
    mock.assert_called_with(1)  # not a test assertion
    self.assertion_helper()     # user helper, not a test assertion
    self.assertEqual(a, b)      # counts
