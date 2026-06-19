"""A realistic module mixing deeply nested literals with ordinary shallow data.

Used by the CLI end-to-end test to confirm SLP084 fires only on the over-nested
data-structure literals when run through the real binary.
"""


# --- over-nested literals (should be flagged) --------------------------------

# A config blob a model would happily emit inline instead of as named types.
SETTINGS = {
    "service": {
        "endpoints": {
            "primary": [{"host": "a", "port": 1}],
        },
    },
}

GRID = [[[[0, 1], [2, 3]]]]

PERMISSIONS = {role: [[[action]] for action in actions] for role in roles}


# --- ordinary shallow data (must not be flagged) -----------------------------

FLAGS = {"debug": True, "verbose": False}

POINTS = [(0, 0), (1, 1), (2, 4)]

NESTED_ONCE = {"users": ["alice", "bob"]}

TWO_DEEP = {"group": {"name": "admins"}}


def build_lookup(rows):
    # Deep control flow, shallow data — the SLP082 axis, not SLP084.
    out = {}
    for row in rows:
        if row.active:
            for field in row.fields:
                if field.value:
                    out[field.name] = [field.value]
    return out
