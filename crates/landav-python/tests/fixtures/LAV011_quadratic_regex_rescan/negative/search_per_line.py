"""Pull the leading date off every log line."""

import re

TIMESTAMP = re.compile(r"^\d{4}-\d{2}-\d{2}")


def stamped(lines):
    """One scan per line; the total is linear in the file."""
    out = []
    for line in lines:
        match = TIMESTAMP.search(line)
        if match is not None:
            out.append(match.group(0))
    return out
