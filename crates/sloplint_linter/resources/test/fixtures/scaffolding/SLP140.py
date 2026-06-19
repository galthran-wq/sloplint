"""A small library module that shipped with its tutorial demo block attached."""


class Pipeline:
    def __init__(self, steps):
        self.steps = steps

    def run(self, data):
        for step in self.steps:
            data = step(data)
        return data


def build_default():
    return Pipeline([])


if __name__ == "__main__":
    # Example usage
    pipeline = build_default()
    print(pipeline.run([1, 2, 3]))
