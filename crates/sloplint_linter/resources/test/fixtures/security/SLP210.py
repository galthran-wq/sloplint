"""Fixture for SLP210 — phantom security-guard calls and decorators."""

from auth import login_required  # defined elsewhere → bound here


def real_guard(token):
    return bool(token)


# --- Violations: guard names that are never defined or imported in this module ---


def handle_request(token):
    if not validate_token(token):  # SLP210: undefined guard call
        raise PermissionError
    data = sanitize_input(token)  # SLP210: undefined guard call
    return data


@requires_auth  # SLP210: undefined guard decorator
def admin_panel():
    return "secret"


@rate_limit(per_minute=5)  # SLP210: undefined guard decorator-call
def public_api():
    return "ok"


# --- Near-miss: a bound name one edit away → reported as a likely typo, not pure phantom ---


def sanitise(value):
    return value.strip()


def store(value):
    return sanitize(value)  # SLP210 (typo of the local `sanitise`)


# --- Non-violations ---


@login_required  # imported above → bound, not flagged
def dashboard():
    return "ok"


def safe(token):
    # A locally-defined guard is fine.
    if not real_guard(token):
        raise PermissionError
    # An attribute call resolves via its receiver — never flagged.
    return validators.validate_token(token)


def not_a_guard():
    # A plain undefined call that isn't a known security guard is Ruff's F821, not SLP210.
    return compute_total()
