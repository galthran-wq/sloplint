"""Production module."""


class TokenBucket:
    def take(self, amount):
        return amount


def build_default():
    return TokenBucket()


def reset_state():
    return None
