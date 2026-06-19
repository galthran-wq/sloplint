"""A realistic-looking module that shipped with one leaked template placeholder.

This fixture lives outside any `tests/` directory on purpose: SLP100 self-exempts
test/example/docs paths, so the end-to-end test drives the real binary over a normal
source path.
"""

API_KEY = "your_api_key_here"
CUSTOM_SLOT = "please fill_me_in before shipping"


def total(values):
    return sum(values)
