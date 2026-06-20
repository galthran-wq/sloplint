"""Test module — measured in the test panel, never the production one (#96).

It imports the production package; that import must NOT show up as production coupling
(`app.imported_by`) when the import graph is built from production modules only.
"""

from app.core import build


def test_runs():
    engine = build()
    assert engine.run([1]) == 1
    assert engine.run([]) == 0


def test_negative():
    assert build().run([-1]) == 0
