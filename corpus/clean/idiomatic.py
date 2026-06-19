def normalize(values):
    total = sum(values)
    if total == 0:
        return values
    return [value / total for value in values]


def chunk(items, size):
    return [items[i : i + size] for i in range(0, len(items), size)]
