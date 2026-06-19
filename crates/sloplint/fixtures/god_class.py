"""Fixture for the SLP120 god-class e2e test."""


class ReportService:
    def fetch_rows(self, query):
        return self.db.execute(query)

    def count_rows(self, query):
        return self.db.count(query)

    def render_html(self, rows):
        return self.template.render(rows)

    def render_pdf(self, rows):
        return self.template.to_pdf(rows)


class Accumulator:
    def __init__(self):
        self.items = []

    def add(self, item):
        self.items.append(item)

    def total(self):
        return sum(self.items)

    def clear(self):
        self.items = []
