"""Two accumulators, one name, two different types."""


def render_flags(state):
    """A genuine string accumulator, over a fixed two-element loop."""
    result = ""
    for flag in ("stale", "dirty"):
        if getattr(state, flag, False):
            result += flag[0]
    return result


def collect_errors(records):
    """A *list* accumulator that happens to reuse the name ``result``.

    ``list.__iadd__`` extends in place and is amortised O(1) per element, so
    this loop is linear. Nothing here copies a string.
    """
    result = []
    for record in records:
        result += record.errors
    return result
