"""Keep the best scores from a stream that does not fit in memory."""

_TOP_N = 10


def top_scores(stream):
    """``top`` never exceeds ten entries, so each sort is constant work.

    Sorting once after the loop is not an option: it would mean holding the
    whole stream. Truncating after every insert is what keeps the memory bound,
    and the sort it needs costs O(_TOP_N log _TOP_N) per item.
    """
    top = []
    for entry in stream:
        top.append((entry.score, entry.name))
        top.sort(reverse=True)
        del top[_TOP_N:]
    return top
