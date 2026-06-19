def reraise(path):
    try:
        return open(path).read()
    except Exception as exc:
        raise exc


def swallow(path):
    try:
        return open(path).read()
    except Exception:
        pass


def log_only(path):
    try:
        return open(path).read()
    except Exception:
        logging.error("could not read")


def specific(path):
    try:
        return open(path).read()
    except FileNotFoundError:
        return ""


def log_and_reraise(path):
    try:
        return open(path).read()
    except Exception:
        logging.exception("could not read")
        raise


def translate(path):
    try:
        return open(path).read()
    except Exception as exc:
        raise RuntimeError("could not read") from exc
