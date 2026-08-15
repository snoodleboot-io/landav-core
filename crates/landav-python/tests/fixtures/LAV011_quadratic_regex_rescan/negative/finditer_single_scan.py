"""Find every match in one left-to-right pass."""

import re


def find_all_spans(pattern, text):
    """One scan of the document."""
    return [(match.start(), match.end()) for match in re.finditer(pattern, text)]
