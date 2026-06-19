"""Example client module. The module docstring mentions no placeholders here."""

# A real, tracked follow-up — this is Ruff's flake8-todos territory, not ours.
# TODO: refactor this once the new endpoint lands

# --- violations: leftover template residue ---

API_KEY = "your_api_key_here"
auth_token = "your_token_here"


def fetch(url):
    # Insert your code here
    return None


def handle(event):
    # Replace this with your handler logic
    return None


def render(name):
    return f"signed in as your_api_key_here"


# --- non-violations: ordinary code and a tracked tag ---


def add(a, b):
    # accumulate the running total before returning
    return a + b


GREETING = "Welcome back to your account dashboard"
RETRY_NOTE = "FIXME later"  # bare tag only — Ruff's job, not SLP100
