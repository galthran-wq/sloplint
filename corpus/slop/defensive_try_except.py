def read_file(path):
    try:
        with open(path) as handle:
            return handle.read()
    except Exception as exc:
        # log and re-raise without adding any value
        raise exc


def to_int(text):
    try:
        return int(text)
    except Exception:
        return None
