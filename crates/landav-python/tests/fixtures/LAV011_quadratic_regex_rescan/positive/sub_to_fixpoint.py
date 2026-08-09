"""Collapse runs of blank lines."""

import re


def collapse_blank_lines(text):
    """Each pass is linear and the number of passes grows with the input."""
    previous = None
    while previous != text:
        previous = text
        text = re.sub(r"\n\n\n", "\n\n", text)  # LANDAV: LAV011 anchor=re.sub(r"\n\n\n", "\n\n", text)
    return text
