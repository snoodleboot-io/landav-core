"""Pull request ids out of log lines that carry a fixed-width timestamp."""

import re

_ID_RE = re.compile(r"req-[0-9a-f]{12}")
_TIMESTAMP_WIDTH = 24


def request_ids(lines):
    """The slice is of *this line*, not of the whole input.

    Every byte of the input is scanned once across the whole loop, so the
    function is linear. The slice skips a fixed-width timestamp that can never
    contain an id; it does not re-scan anything an earlier iteration already
    looked at.
    """
    found = []
    for line in lines:
        match = _ID_RE.search(line[_TIMESTAMP_WIDTH:])
        if match:
            found.append(match.group(0))
    return found
