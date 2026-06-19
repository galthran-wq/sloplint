def deep(rows):
    for a in rows:
        for b in a:
            for c in b:
                for d in c:
                    if d:
                        return d
    return None


def shallow(value):
    if value:
        return 1
    return 0
