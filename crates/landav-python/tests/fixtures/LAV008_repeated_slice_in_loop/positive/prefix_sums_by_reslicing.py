"""Prefix sums, the accidentally quadratic way."""


def prefix_sums(values):
    """The slice grows with ``i``, so the copies alone are quadratic."""
    sums = []
    for i in range(len(values)):
        sums.append(sum(values[:i + 1]))  # LANDAV: LAV008 anchor=values[:i + 1]
    return sums
