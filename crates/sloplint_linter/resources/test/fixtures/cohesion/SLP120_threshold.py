"""Threshold fixture for SLP120: a two-concept god class, judged under varying limits."""


class Utils:
    def parse(self, text):
        return self.parser.run(text)

    def tokenize(self, text):
        return self.parser.split(text)

    def render(self, node):
        return self.formatter.render(node)
