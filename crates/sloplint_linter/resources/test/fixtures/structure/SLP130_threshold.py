# A 4-branch literal-dispatch ladder used to pin the configurable branch limit: flagged when
# `dispatch_max_branches` is below 4, silent when it is raised to 4 (the chain must exceed it).
def route(kind):
    if kind == "a":
        return 1
    elif kind == "b":
        return 2
    elif kind == "c":
        return 3
    elif kind == "d":
        return 4
