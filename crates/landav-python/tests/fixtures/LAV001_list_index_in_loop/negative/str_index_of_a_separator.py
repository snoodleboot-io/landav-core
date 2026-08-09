"""Split ``key=value`` configuration lines."""


def parse_settings(lines):
    """``str.index`` scans one line, not one collection.

    The cost of ``line.index("=")`` is bounded by the length of *this* line, so
    the loop copies and scans each byte of the input exactly once and the total
    is linear. There is no position map to build: the separator offset differs
    per line and is used immediately.
    """
    settings = {}
    for line in lines:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        cut = stripped.index("=")
        settings[stripped[:cut].rstrip()] = stripped[cut + 1:].lstrip()
    return settings
