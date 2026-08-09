"""Pull the timestamp off every log line."""


def timestamps(lines):
    """A nineteen-byte slice is O(1) whatever the line length."""
    stamps = []
    for line in lines:
        stamps.append(line[:19])
    return stamps
