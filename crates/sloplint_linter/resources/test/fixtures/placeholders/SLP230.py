"""Fixture for SLP230 — mock / placeholder data in production code."""

# --- Violations ---

ADMIN_EMAIL = "admin@example.com"  # placeholder email
support = "Help <help@example.net>"  # placeholder email
NIL_ID = "00000000-0000-0000-0000-000000000000"  # nil UUID
SAMPLE_ID = "11111111-1111-1111-1111-111111111111"  # low-entropy UUID
CONTACT = "123-456-7890"  # placeholder phone

password = "changeme"  # weak credential
api_key = "your_api_key"  # weak credential
DB_SECRET = "password123"  # weak credential


def get_user():
    return {"foo": "bar"}  # dummy return dict


def fetch_config():
    return "placeholder"  # dummy return string


def connect():
    return client.connect(password="test123")  # weak credential via keyword


# --- Non-violations ---

REAL_EMAIL = "ops@acme-prod.io"  # real domain (not in placeholder set)
REAL_ID = "f47ac10b-58cc-4372-a567-0e02b2c3d479"  # real high-entropy UUID
REAL_PHONE = "415-826-3199"  # realistic number
strong_password = "a7Fq9zLp2KdM"  # looks real
MAX_RETRIES = 5  # ordinary constant


def real_logic(x):
    return {"id": x.id, "total": x.total}  # real dict, not placeholder tokens


def greet(name):
    return f"Hello, {name}"  # real string return


def documented():
    """Send a note to user@example.com as an example.

    A docstring example email is documentation, not slop — not flagged.
    """
    return name
