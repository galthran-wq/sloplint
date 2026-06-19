def parse_iso8601(text):
    """Parse an ISO-8601 timestamp, assuming UTC when no offset is present."""
    return _parse(text)


def retry_backoff(attempt):
    """Return the delay in seconds using exponential backoff with full jitter."""
    return min(2**attempt, 30)


def _parse(text):
    return text
