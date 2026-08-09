"""Collapse runs of blank lines in one pass."""

import re

BLANK_RUN = re.compile(r"\n{3,}")


def collapse_blank_lines(text):
    """One pass, with the repetition expressed in the pattern."""
    return BLANK_RUN.sub("\n\n", text)
