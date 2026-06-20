"""Production module — measured in the production panel (#96)."""


class Engine:
    def run(self, items):
        for item in items:
            if item and item > 0:
                return item
        return 0


def build():
    return Engine()
