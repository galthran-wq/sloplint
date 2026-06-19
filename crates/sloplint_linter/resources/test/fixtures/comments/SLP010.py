#!/usr/bin/env python
# this standalone comment is banned prose
import os  # inline prose comment, also banned

pid = os.getpid()  # noqa: F401
count: int = 0  # type: ignore
# TODO(PROJ-123): wire this up properly
# TODO figure this out somehow
value = pid + count
