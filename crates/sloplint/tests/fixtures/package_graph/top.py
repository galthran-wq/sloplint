"""A top-level module (no package) with only third-party / stdlib imports.

It lands in the root package `.` and has no first-party coupling.
"""

import os


def main():
    return os.getcwd()
