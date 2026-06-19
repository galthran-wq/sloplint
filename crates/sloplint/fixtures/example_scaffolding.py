"""A library module whose tutorial demo block was left attached."""


class TokenBucket:
    def __init__(self, capacity):
        self.capacity = capacity
        self.tokens = capacity

    def take(self, n):
        if n > self.tokens:
            return False
        self.tokens -= n
        return True


if __name__ == "__main__":
    # Example usage
    bucket = TokenBucket(10)
    print(bucket.take(3))
    print(bucket.take(8))
