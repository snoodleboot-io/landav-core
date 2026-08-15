"""Total and peak of a value list."""


def summarise(values):
    """Two loops in sequence over the same list: still linear."""
    total = 0
    for value in values:
        total += value
    peak = 0
    for value in values:
        peak = max(peak, value)
    return total, peak
