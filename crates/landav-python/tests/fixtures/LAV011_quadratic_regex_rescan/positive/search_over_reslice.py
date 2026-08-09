"""Find every match of a pattern in a document."""

import re


def find_all_spans(pattern, text):
    """Each search copies the tail and rescans it from the start."""
    spans = []
    offset = 0
    while offset < len(text):
        match = re.search(pattern, text[offset:])  # LANDAV: LAV011 anchor=re.search(pattern, text[offset:])
        if match is None:
            break
        spans.append((offset + match.start(), offset + match.end()))
        offset += max(match.end(), 1)
    return spans
