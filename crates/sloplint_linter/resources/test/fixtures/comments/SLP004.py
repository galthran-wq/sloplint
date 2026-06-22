# Copyright 2026 Example Corp. Licensed under MIT.  (license header — not a tell)
"""Module docstring — not a comment, never scanned."""

x = 1  # for now, just return a constant
y = 2  # in production this would query the database
z = 3  # this is a placeholder until the real impl lands

a = 4  # should work for most inputs
b = 5  # probably fine, replace this with your actual config

# Step 1: parse the input
# Phase 2 - transform the records
# ============================
# This function handles the request lifecycle


def real(n):
    # because the upstream API rejects empty payloads, guard here  (legitimate WHY)
    # see RFC 2606 for reserved domains  (reference, not a tell)
    return n + 1  # noqa: E501
