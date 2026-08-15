"""Find every match by moving a start offset."""

import re


def find_all_spans(pattern, text):
    """``pos`` moves the start without copying; this is the fix for the rule."""
    compiled = re.compile(pattern)
    spans = []
    offset = 0
    while True:
        match = compiled.search(text, offset)
        if match is None:
            break
        spans.append((match.start(), match.end()))
        offset = max(match.end(), match.start() + 1)
    return spans
